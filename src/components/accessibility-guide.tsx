import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";

// アクセシビリティ設定ペインへの直接リンク（macOS のシステム設定 URL スキーム）
const ACCESSIBILITY_PANE_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/**
 * アクセシビリティ権限のガイド画面。
 *
 * `check_accessibility` で権限を確認し、未許可のときだけ children の代わりに
 * 設定への導線つきガイドを表示する。再確認ボタン、およびウィンドウへフォーカスが
 * 戻ったタイミングで自動再チェックし、許可後すぐに本来の画面へ復帰する。
 */
export function AccessibilityGuide({ children }: { children: React.ReactNode }) {
  // null = 確認中
  const [trusted, setTrusted] = useState<boolean | null>(null);

  const recheck = useCallback(async () => {
    try {
      setTrusted(await invoke<boolean>("check_accessibility"));
    } catch {
      // コマンド失敗時はガイドを出して導線を示す
      setTrusted(false);
    }
  }, []);

  useEffect(() => {
    recheck();
    // 設定アプリから戻ってきたら再確認
    window.addEventListener("focus", recheck);
    return () => window.removeEventListener("focus", recheck);
  }, [recheck]);

  if (trusted === null) return null;
  if (trusted) return <>{children}</>;

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="space-y-1">
        <h2 className="text-base font-semibold">アクセシビリティ権限が必要です</h2>
        <p className="text-sm text-muted-foreground">
          選択中のテキストを取得するために cmd+C を擬似送信します。
          システム設定で本アプリ（開発時はターミナル/Tauri
          devプロセス）にアクセシビリティ権限を許可してください。
        </p>
      </div>

      <ol className="list-decimal space-y-1 pl-5 text-sm text-muted-foreground">
        <li>下のボタンでアクセシビリティ設定を開く</li>
        <li>リストで本アプリのスイッチをオンにする</li>
        <li>「再確認」を押す（設定から戻ると自動で再確認します）</li>
      </ol>

      <div className="flex gap-2">
        <Button onClick={() => openUrl(ACCESSIBILITY_PANE_URL)}>
          アクセシビリティ設定を開く
        </Button>
        <Button variant="outline" onClick={recheck}>
          再確認
        </Button>
      </div>
    </div>
  );
}
