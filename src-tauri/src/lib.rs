use std::path::PathBuf;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow,
};

const SYSTEM_PROMPT: &str = "Translate the English text to Japanese. \
    Output ONLY the Japanese translation. No preamble, no explanations.";

// 入力の上限。これを超えた分は切り捨て、フロントへ警告を出す。
const MAX_INPUT_CHARS: usize = 5000;

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    model: String,
    endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_y: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: "qwen2.5:14b".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            window_x: None,
            window_y: None,
        }
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".config")
        .join("local-ai-translator")
        .join("config.json")
}

fn load_config_internal() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn load_config() -> Config {
    load_config_internal()
}

fn save_config_internal(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn save_config(config: Config) -> Result<(), String> {
    save_config_internal(&config)
}

// Ollama /api/chat のストリーミング応答 1 行分。
#[derive(Deserialize)]
struct ChatStreamChunk {
    message: Option<ChatMessage>,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

/// Ollama の /api/chat へストリーミング POST し、届いたトークンを
/// `translate-token` イベントでフロントへ逐次送信する。完了で `translate-done`。
#[tauri::command]
async fn translate<R: Runtime>(window: tauri::Window<R>, text: String) -> Result<(), String> {
    let cfg = load_config_internal();

    let mut input = text;
    if input.chars().count() > MAX_INPUT_CHARS {
        input = input.chars().take(MAX_INPUT_CHARS).collect();
        let _ = window.emit(
            "translate-warning",
            format!("入力が {MAX_INPUT_CHARS} 文字を超えたため切り詰めました。"),
        );
    }

    let payload = json!({
        "model": cfg.model,
        "stream": true,
        "keep_alive": "30m",
        "options": { "temperature": 0.2 },
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": input }
        ]
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(format!("{}/api/chat", cfg.endpoint))
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            format!("Ollama に接続できません（{e}）。`brew services start ollama` で起動してください。")
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        // モデル未 pull は 404 で返ってくる。
        if status.as_u16() == 404 {
            return Err(format!(
                "モデルが見つかりません（{}）。`ollama pull {}` で取得してください。",
                detail, cfg.model
            ));
        }
        return Err(format!("Ollama がエラーを返しました（{status}）: {detail}"));
    }

    let mut resp = resp;
    let mut buf = String::new();
    loop {
        match resp.chunk().await {
            Ok(Some(bytes)) => {
                buf.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(idx) = buf.find('\n') {
                    let line: String = buf.drain(..=idx).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(line) {
                        if let Some(msg) = chunk.message {
                            if !msg.content.is_empty() {
                                let _ = window.emit("translate-token", msg.content);
                            }
                        }
                        if chunk.done {
                            let _ = window.emit("translate-done", ());
                            return Ok(());
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => return Err(format!("応答の受信に失敗しました: {e}")),
        }
    }

    let _ = window.emit("translate-done", ());
    Ok(())
}

/// 起動時にモデルをメモリへ先読みし、初回 cmd+j のロード待ちを消す。
/// messages を空にした /api/chat はモデルをロードするだけで応答生成しない。
#[tauri::command]
async fn warm_model() -> Result<(), String> {
    let cfg = load_config_internal();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let payload = json!({ "model": cfg.model, "keep_alive": "30m", "messages": [] });
    let _ = client
        .post(format!("{}/api/chat", cfg.endpoint))
        .json(&payload)
        .send()
        .await;
    Ok(())
}

// AXIsProcessTrusted は macOS の ApplicationServices フレームワークが提供する。
// 現プロセスがアクセシビリティ権限を持つか（Boolean = unsigned char）を返す。
#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

/// 現プロセスにアクセシビリティ権限が付与されているかを返す。
/// cmd+C 擬似入力（grab_selection）に必須。macOS 以外では常に true。
#[tauri::command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { AXIsProcessTrusted() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// 現在選択中のテキストを取得する。
///
/// クリップボードを退避 → cmd+C を擬似送信 → 退避値から内容が変化するまで
/// 最大 500ms ポーリングして読み取り → 退避内容を復元する。固定 sleep ではなく
/// 変化検知でポーリングすることで Teams/ブラウザなどコピー反映が遅いアプリにも追従する。
/// 取得できなければ空文字を返す。
#[tauri::command]
fn grab_selection() -> Result<String, String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("クリップボード初期化失敗: {e}"))?;

    // 退避（空 or 非テキストのときは None）
    let saved = clipboard.get_text().ok();

    // cmd+C を擬似送信
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("enigo 初期化失敗: {e}"))?;
    enigo
        .key(Key::Meta, Press)
        .map_err(|e| format!("cmd 押下失敗: {e}"))?;
    let click_result = enigo.key(Key::Unicode('c'), Click);
    let _ = enigo.key(Key::Meta, Release); // 押しっぱなしを避けるため Release は必ず実行
    click_result.map_err(|e| format!("c キー送信失敗: {e}"))?;

    // 退避値から変化するまで最大 500ms ポーリング
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut grabbed: Option<String> = None;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        if let Ok(current) = clipboard.get_text() {
            if Some(&current) != saved.as_ref() && !current.is_empty() {
                grabbed = Some(current);
                break;
            }
        }
    }

    // 退避内容を復元
    match &saved {
        Some(text) => {
            let _ = clipboard.set_text(text.clone());
        }
        None => {
            let _ = clipboard.clear();
        }
    }

    Ok(grabbed.unwrap_or_default())
}

/// 画面中央へウィンドウを移動する（初回表示時のデフォルト位置）。
fn position_center<R: Runtime>(window: &WebviewWindow<R>) {
    let (Ok(Some(monitor)), Ok(win_size)) = (window.current_monitor(), window.outer_size()) else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let x = origin.x + (screen.width as i32 - win_size.width as i32) / 2;
    let y = origin.y + (screen.height as i32 - win_size.height as i32) / 2;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// 保存済み位置があれば復元、なければ画面中央へ配置してから表示しフォーカスする。
fn show_window<R: Runtime>(window: &WebviewWindow<R>) {
    let config = load_config_internal();
    if let (Some(x), Some(y)) = (config.window_x, config.window_y) {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    } else {
        position_center(window);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// フロントエンドの×ボタンから呼ばれる。位置を保存してからウィンドウを隠す。
#[tauri::command]
fn hide_window<R: Runtime>(window: tauri::Window<R>) -> Result<(), String> {
    if let Ok(pos) = window.outer_position() {
        let mut config = load_config_internal();
        config.window_x = Some(pos.x);
        config.window_y = Some(pos.y);
        save_config_internal(&config)?;
    }
    let _ = window.hide();
    let _ = window.emit("reset", ());
    Ok(())
}

/// cmd+j のトグル挙動。表示中なら隠し、非表示なら
/// 選択取得 → 表示 → `translate-request` イベントでフロントへ通知する。
/// 選択が空でもエラーにせず空文字を渡す（フロントが手動入力モードで表示する）。
fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        if let Ok(pos) = window.outer_position() {
            let mut config = load_config_internal();
            config.window_x = Some(pos.x);
            config.window_y = Some(pos.y);
            let _ = save_config_internal(&config);
        }
        let _ = window.hide();
        let _ = window.emit("reset", ());
    } else {
        // ウィンドウ表示前（＝元アプリにフォーカスがある間）に選択を取得する。
        let selection = grab_selection().unwrap_or_default();
        show_window(&window);
        let _ = window.emit("translate-request", selection);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
                    if event.state() == ShortcutState::Pressed
                        && shortcut.matches(Modifiers::SUPER, Code::KeyJ)
                    {
                        toggle_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
                let cmd_j = Shortcut::new(Some(Modifiers::SUPER), Code::KeyJ);
                app.global_shortcut().register(cmd_j)?;
            }

            // Dock には出さず、メニューバー常駐のみとする（skipTaskbar と整合）。
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // メニューバー常駐のトレイアイコン。メニューから設定・終了を出す。
            let settings = MenuItem::with_id(app, "settings", "設定…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Local AI Translator")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(window) = app.get_webview_window("main") {
                            show_window(&window);
                            let _ = window.emit("open-settings", ());
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            hide_window,
            translate,
            warm_model,
            check_accessibility,
            grab_selection
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
