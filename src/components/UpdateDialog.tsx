import { Download, Loader2 } from "lucide-react";
import type { AppUpdateSummary } from "../lib/appUpdater";

export function UpdateDialog({
  currentVersion,
  update,
  busy,
  progress,
  error,
  onInstall,
  onDismiss
}: {
  currentVersion: string;
  update: AppUpdateSummary;
  busy: boolean;
  progress: number;
  error: string | null;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  const releaseNotes = update.body?.trim();

  return (
    <div className="oauthDialogBackdrop">
      <div className="oauthDialog updateDialog" role="dialog" aria-modal="true" aria-labelledby="updateDialogTitle">
        <div className="oauthDialogHeader">
          <div>
            <Download className="oauthDialogIcon" size={20} />
            <h2 id="updateDialogTitle">发现新版本</h2>
          </div>
        </div>
        <div className="oauthDialogBody updateDialogBody">
          <div className="updateDialogVersions">
            <div className="updateDialogVersionRow">
              <span>当前版本</span>
              <strong>v{currentVersion}</strong>
            </div>
            <div className="updateDialogVersionRow updateDialogVersionRowNext">
              <span>更新版本</span>
              <strong>v{update.version}</strong>
            </div>
          </div>

          <div className="updateDialogSection">
            <span className="updateDialogSectionLabel">更新内容</span>
            {releaseNotes ? (
              <pre className="updateDialogNotes">{releaseNotes}</pre>
            ) : (
              <p className="updateDialogHint">本次更新暂无详细说明。</p>
            )}
          </div>

          {busy && (
            <div className="updateDialogProgress">
              <div className="updateDialogProgressTrack">
                <div className="updateDialogProgressBar" style={{ width: `${progress}%` }} />
              </div>
              <span>{progress > 0 ? `下载中 ${progress}%` : "准备下载..."}</span>
            </div>
          )}
          {error && <div className="formError">{error}</div>}
        </div>
        <div className="oauthDialogFooter">
          <button className="button" type="button" disabled={busy} onClick={onDismiss}>
            稍后更新
          </button>
          <button className="button primary" type="button" disabled={busy} onClick={onInstall}>
            {busy ? <Loader2 className="spin" size={16} /> : <Download size={16} />}
            立即更新
          </button>
        </div>
      </div>
    </div>
  );
}
