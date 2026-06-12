# Local MS Translator — タスクリスト

実装は #4 → #14 の順で進める。
#1 の Ollama+モデルは #8（翻訳テスト）までに、#1b の Rust は #4（初期化）までに、
#3 の権限は #14（検証）までに揃っていればよい。

## 環境セットアップ（Claude が実行）

- [x] **#1 Ollama 導入 + モデル pull**
  `brew install ollama` で導入し、`brew services start ollama` でバックグラウンド常駐させる。`ollama pull qwen2.5:14b` で翻訳モデルを取得（ロード時RAM約9GB。軽量化したいなら `qwen2.5:7b` / `qwen2.5:3b`）。`curl http://localhost:11434/api/tags` で起動を確認。
- [x] **#1b Rust ツールチェーン導入**
  #4 の前提。rustup で導入（`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`）。`cargo --version` が通ることを確認。

## ユーザー側の事前準備

- [ ] **#2 GitHub private リポジトリ作成**
  local-ms-translator 用の private リポジトリを作成し、リモートを設定（`gh repo create` でも可）。
- [ ] **#3 アクセシビリティ権限の許可**
  実機テスト時に、システム設定 > プライバシーとセキュリティ > アクセシビリティ で本アプリ（開発時はターミナル/Tauri dev プロセス）に許可を与える。cmd+C 擬似入力に必須。実装完了後の検証ステップで実施。

## 実装タスク

- [x] **#4 Tauri v2 プロジェクト初期化**
  `pnpm create tauri-app` で React + TypeScript テンプレートを生成。Vite + React + TS のフロント、src-tauri の Rust バックエンドを用意。`pnpm tauri dev` が起動することを確認。
- [x] **#5 Tailwind + shadcn/ui セットアップ**
  Tailwind を導入し shadcn/ui を初期化。Button / Input / Dialog / Textarea などを追加。
- [x] **#6 Rust 依存とプラグイン追加**
  src-tauri に `tauri-plugin-global-shortcut`, `enigo`, `arboard`（または clipboard-manager プラグイン）, `reqwest`, `serde` を追加。tauri.conf.json の capabilities/permissions を整備。
- [x] **#7 設定ファイル読み書き + 設定ダイアログ**
  Rust の `load_config`/`save_config` で `~/.config/local-ms-translator/config.json` を読み書き（model, endpoint）。既定は model=`qwen2.5:14b`, endpoint=`http://localhost:11434`。フロントに歯車ボタン → shadcn Dialog でモデル名・エンドポイントのテキスト入力フォーム（モデルは通常固定のため一覧取得はしない）。
- [x] **#8 翻訳コマンド（Ollama・ストリーミング）**
  Rust の `translate(text)` で Ollama の `{endpoint}/api/chat` へ POST（`stream:true`、`keep_alive` 長め=30m、low temperature、system prompt で「英文を自然な日本語に訳し訳文のみ返す」を指示）。届いたトークンを Tauri イベント/Channel でフロントへ逐次送信する。`reqwest`/`serde` を使用。リクエストは 30 秒タイムアウト、入力は 5000 文字で打ち切り＆警告。エラーハンドoリング（Ollama 未起動＝接続不可、モデル未 pull、応答失敗）も実装。
  起動時にモデルを先読みする `warm_model`（空に近いリクエスト or `keep_alive` 指定の load）も用意し、初回 cmd+j のロード待ちを消す。
- [x] **#9 選択テキスト取得コマンド `grab_selection`**
  現在のクリップボードを退避 → enigo で cmd+C 擬似送信 → **退避値から内容が変化するまで最大 ~500ms ポーリング**してクリップボード読取（固定 sleep にしない）→ 文字列を返し → 退避内容を復元。Teams/ブラウザで動作する方式。取得できなければ空文字を返す。
- [x] **#10 アクセシビリティ権限チェック + ガイド画面**
  Rust の `check_accessibility`（`AXIsProcessTrusted`）。フロントで false のときガイド画面を表示し、システム設定アクセシビリティへの導線を出す。加えて #8 で Ollama 未起動時はその旨をフロントへ伝え、起動方法（`brew services start ollama`）を案内する。
- [x] **#11 グローバルショートカット cmd+j とトグル挙動**
  `tauri-plugin-global-shortcut` で cmd+j を登録。表示中なら hide、非表示なら「表示 → grab_selection → translate → 結果をストリーミング表示」のフローを起動。**選択テキストが空のときはエラーにせず、空の入力欄にフォーカスした手動入力モードで表示**する。非表示時に入力・結果をリセット。
- [x] **#12 Spotlight 風ウィンドウ設定**
  tauri.conf.json で `decorations:false`, `alwaysOnTop:true`, `skipTaskbar:true`。幅 720px・横長で、訳文量に応じて高さを自動伸長（上限あり、超過分はスクロール）。表示時に Rust 側で画面中央やや上へ `set_position`。
- [x] **#12b メニューバー常駐**
  Tauri v2 の tray-icon でメニューバーにアイコンを常駐させ、メニューから設定・終了を出す。Dock には出さない（`skipTaskbar` と整合）。
- [x] **#13 メイン UI 実装**
  テキストエリア（直接入力）／翻訳ボタン／訳文表示（ストリーミングで逐次描画）／コピーボタン（押下時のみクリップボードへ）／設定ボタン／閉じるボタンを shadcn で実装。ウィンドウ表示イベント受信で自動翻訳フロー（選択が空なら手動入力モード）、非表示でリセット。
- [ ] **#14 E2E 検証**
  `pnpm tauri dev` で起動し、(1) 設定保存（model/endpoint）→ config.json 確認、(2) 直接入力翻訳、(3) Teams/ブラウザ選択 → cmd+j 自動翻訳＆クリップボード復元確認、(4) 再 cmd+j / 閉じるでリセット、(5) 権限未許可でガイド表示、(6) 機内モード等オフラインでも翻訳できること（完全ローカルの証明）、を確認。
