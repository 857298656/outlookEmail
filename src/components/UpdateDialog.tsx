import { CheckCircle2, Download, Loader2, RefreshCw, X } from "lucide-react";
import type { AppUpdateSummary } from "../lib/appUpdater";

export function UpdateDialog({
  currentVersion,
  update,
  checking,
  checkComplete,
  busy,
  progress,
  error,
  onInstall,
  onDismiss
}: {
  currentVersion: string;
  update: AppUpdateSummary | null;
  checking: boolean;
  checkComplete: boolean;
  busy: boolean;
  progress: number;
  error: string | null;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  const releaseNotes = update?.body?.trim();
  const title = update ? "发现新版本" : "检查更新";

  return (
    <div className="oauthDialogBackdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !checking && !busy) onDismiss();
    }}>
      <div className="oauthDialog updateDialog" role="dialog" aria-modal="true" aria-labelledby="updateDialogTitle">
        <div className="oauthDialogHeader">
          <div>
            {checking ? <Loader2 className="oauthDialogIcon spin" size={20} /> : update ? <Download className="oauthDialogIcon" size={20} /> : <RefreshCw className="oauthDialogIcon" size={20} />}
            <h2 id="updateDialogTitle">{title}</h2>
          </div>
          <button className="iconMini" type="button" title="关闭" aria-label="关闭" disabled={checking || busy} onClick={onDismiss}>
            <X size={18} />
          </button>
        </div>

        <div className="oauthDialogBody updateDialogBody">
          <div className="updateDialogVersions">
            <div className="updateDialogVersionRow">
              <span>当前版本</span>
              <strong>v{currentVersion}</strong>
            </div>
            {update && (
              <div className="updateDialogVersionRow updateDialogVersionRowNext">
                <span>更新版本</span>
                <strong>v{update.version}</strong>
              </div>
            )}
          </div>

          {checking && (
            <div className="updateCheckState">
              <Loader2 className="spin" size={24} />
              <div><strong>正在检查更新</strong></div>
            </div>
          )}

          {!checking && checkComplete && !update && !error && (
            <div className="updateCheckState success">
              <CheckCircle2 size={24} />
              <div><strong>当前已是最新版本</strong><span>暂时没有可用的新版本。</span></div>
            </div>
          )}

          {update && (
            <div className="updateDialogSection">
              <span className="updateDialogSectionLabel">更新内容</span>
              {releaseNotes ? <pre className="updateDialogNotes">{releaseNotes}</pre> : <p className="updateDialogHint">本次更新暂无详细说明。</p>}
            </div>
          )}

          {busy && (
            <div className="updateDialogProgress">
              <div className="updateDialogProgressTrack">
                <div className="updateDialogProgressBar" style={{ width: `${progress}%` }} />
              </div>
              <span>{progress > 0 ? `下载中 ${progress}%` : "准备下载…"}</span>
            </div>
          )}
          {error && <div className="formError">检查更新失败：{error}</div>}
        </div>

        <div className="oauthDialogFooter">
          <button className="button" type="button" disabled={checking || busy} onClick={onDismiss}>
            {update ? "稍后更新" : "关闭"}
          </button>
          {update && (
            <button className="button primary" type="button" disabled={busy} onClick={onInstall}>
              {busy ? <Loader2 className="spin" size={16} /> : <Download size={16} />}
              立即更新
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
