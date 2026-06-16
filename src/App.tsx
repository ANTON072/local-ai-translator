import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Check, Copy, Languages, Settings, X } from "lucide-react";

import { AccessibilityGuide } from "@/components/accessibility-guide";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

type Config = { model: string; endpoint: string };

const DEFAULT_CONFIG: Config = {
  model: "qwen2.5:14b",
  endpoint: "http://localhost:11434",
};

function Translator() {
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [isTranslating, setIsTranslating] = useState(false);
  const [error, setError] = useState("");
  const [warning, setWarning] = useState("");
  const [copied, setCopied] = useState(false);

  const [config, setConfig] = useState<Config>(DEFAULT_CONFIG);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [draftConfig, setDraftConfig] = useState<Config>(DEFAULT_CONFIG);
  const [draftLaunchAtLogin, setDraftLaunchAtLogin] = useState(false);

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const rootRef = useRef<HTMLElement>(null);

  const reset = useCallback(() => {
    setInput("");
    setOutput("");
    setError("");
    setWarning("");
    setCopied(false);
    setIsTranslating(false);
  }, []);

  const runTranslate = useCallback(async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    setOutput("");
    setError("");
    setCopied(false);
    setIsTranslating(true);
    try {
      // 訳文トークンは translate-token イベントで逐次届く（下の useEffect で購読）。
      await invoke("translate", { text: trimmed });
    } catch (e) {
      setError(String(e));
      setIsTranslating(false);
    }
  }, []);

  // 初回: 設定読込 + モデル先読み
  useEffect(() => {
    invoke<Config>("load_config")
      .then((cfg) => {
        setConfig(cfg);
        setDraftConfig(cfg);
      })
      .catch(() => {});
    invoke("warm_model").catch(() => {});
  }, []);

  // ストリーミング / ウィンドウイベントの購読
  useEffect(() => {
    const unlisteners = [
      listen<string>("translate-token", (e) => {
        setOutput((prev) => prev + e.payload);
      }),
      listen("translate-done", () => {
        setIsTranslating(false);
        setOutput((prev) => {
          // モデルが「中国語で翻訳→修正：→日本語」という自己修正パターンを出すことがある。
          // 「修正：」以降の日本語部分だけを残す。
          const correctionMatch = prev.match(/修正[：:]\s*\n?([\s\S]+)$/);
          if (correctionMatch) return correctionMatch[1].trim();

          // ひらがな・カタカナが一切なく CJK 文字だけなら中国語出力と判定してエラーにする。
          const hasKana = /[ぁ-ゖァ-ヶ]/.test(prev);
          const hasCJK = /[一-鿿]/.test(prev);
          if (hasCJK && !hasKana) {
            setError("モデルが中国語を出力しました。もう一度お試しください。");
            return "";
          }

          return prev;
        });
      }),
      listen<string>("translate-warning", (e) => {
        setWarning(e.payload);
      }),
      // cmd+j 表示時: 選択テキストを受け取り、空なら手動入力モード、あれば自動翻訳
      listen<string>("translate-request", (e) => {
        reset();
        const selection = (e.payload ?? "").trim();
        if (selection) {
          setInput(selection);
          void runTranslate(selection);
        } else {
          // 空の入力欄にフォーカスした手動入力モード
          requestAnimationFrame(() => inputRef.current?.focus());
        }
      }),
      // cmd+j で隠したとき / 閉じたときのリセット
      listen("reset", () => {
        reset();
      }),
      // トレイメニューの「設定…」から
      listen("open-settings", () => {
        setDraftConfig((c) => c);
        isEnabled().then(setDraftLaunchAtLogin).catch(() => {});
        setSettingsOpen(true);
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, [reset, runTranslate]);

  // 訳文量に応じてウィンドウ高さを内容にあわせて自動伸長する（幅は 720px 固定）。
  // 出力領域側に上限と overflow を持たせているため、内容高は自然に頭打ちになる。
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const apply = () => {
      const height = Math.ceil(root.scrollHeight);
      void getCurrentWindow().setSize(new LogicalSize(720, height));
    };
    const observer = new ResizeObserver(apply);
    observer.observe(root);
    apply();
    return () => observer.disconnect();
  }, []);

  const handleClose = useCallback(async () => {
    reset();
    await invoke("hide_window");
  }, [reset]);

  const handleCopy = useCallback(async () => {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [output]);

  const handleSaveSettings = useCallback(async () => {
    try {
      await invoke("save_config", { config: draftConfig });
      setConfig(draftConfig);
      if (draftLaunchAtLogin) {
        await enable();
      } else {
        await disable();
      }
      setSettingsOpen(false);
    } catch (e) {
      setError(String(e));
    }
  }, [draftConfig, draftLaunchAtLogin]);

  return (
    <main
      ref={rootRef}
      className="flex w-screen flex-col gap-3 bg-background p-4 text-foreground"
    >
      <div
        className="flex items-center justify-between cursor-move select-none"
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest("button")) return;
          void getCurrentWindow().startDragging();
        }}
      >
        <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
          <Languages className="size-4" />
          Local AI Translator
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="設定"
            onClick={() => {
              setDraftConfig(config);
              isEnabled().then(setDraftLaunchAtLogin).catch(() => {});
              setSettingsOpen(true);
            }}
          >
            <Settings />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="閉じる"
            onClick={handleClose}
          >
            <X />
          </Button>
        </div>
      </div>

      <Textarea
        ref={inputRef}
        value={input}
        onChange={(e) => setInput(e.currentTarget.value)}
        placeholder="翻訳したい英文を入力（cmd+J で選択テキストを自動取得）"
        className="min-h-20"
        onKeyDown={(e) => {
          // cmd+Enter で翻訳実行
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            void runTranslate(input);
          }
        }}
      />

      <div className="flex items-center gap-2">
        <Button
          onClick={() => void runTranslate(input)}
          disabled={isTranslating || !input.trim()}
        >
          <Languages />
          {isTranslating ? "翻訳中…" : "翻訳"}
        </Button>
        {warning && <span className="text-xs text-destructive">{warning}</span>}
      </div>

      {error && (
        <div className="rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {(output || isTranslating) && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground">訳文</span>
            <Button
              variant="outline"
              size="sm"
              onClick={handleCopy}
              disabled={!output}
            >
              {copied ? <Check /> : <Copy />}
              {copied ? "コピーしました" : "コピー"}
            </Button>
          </div>
          <div className="max-h-[360px] overflow-y-auto rounded-lg border border-border bg-muted/30 p-3 text-sm whitespace-pre-wrap">
            {output}
            {isTranslating && (
              <span className="ml-0.5 inline-block animate-pulse">▌</span>
            )}
          </div>
        </div>
      )}

      <Dialog open={settingsOpen} onOpenChange={setSettingsOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>設定</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <label className="flex flex-col gap-1.5">
              <span className="text-sm font-medium">モデル名</span>
              <Input
                value={draftConfig.model}
                onChange={(e) =>
                  setDraftConfig((c) => ({ ...c, model: e.currentTarget.value }))
                }
                placeholder="qwen2.5:14b"
              />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-sm font-medium">エンドポイント</span>
              <Input
                value={draftConfig.endpoint}
                onChange={(e) =>
                  setDraftConfig((c) => ({
                    ...c,
                    endpoint: e.currentTarget.value,
                  }))
                }
                placeholder="http://localhost:11434"
              />
            </label>
            <label className="flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={draftLaunchAtLogin}
                onChange={(e) => setDraftLaunchAtLogin(e.currentTarget.checked)}
                className="size-4 cursor-pointer"
              />
              <span className="text-sm font-medium">ログイン時に起動</span>
            </label>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setSettingsOpen(false)}>
              キャンセル
            </Button>
            <Button onClick={handleSaveSettings}>保存</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </main>
  );
}

function App() {
  return (
    <AccessibilityGuide>
      <Translator />
    </AccessibilityGuide>
  );
}

export default App;
