import { useCallback, useEffect, useMemo, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Alert, Modal, Progress, Typography } from "antd";
import MarkdownIt from "markdown-it";

let automaticCheckStarted = false;

type DownloadState = {
  downloaded: number;
  total?: number;
};

export function AutoUpdater() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<DownloadState>({ downloaded: 0 });
  const [error, setError] = useState("");

  const renderedBody = useMemo(() => {
    if (!update?.body) return "";
    const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
    return md.render(update.body);
  }, [update?.body]);

  useEffect(() => {
    if (!isTauri() || automaticCheckStarted) return;
    automaticCheckStarted = true;

    void check({ timeout: 15_000 })
      .then((availableUpdate) => setUpdate(availableUpdate))
      .catch((reason: unknown) => {
        // An unavailable update service must never prevent the app from starting.
        console.warn("自动更新检查失败", reason);
      });
  }, []);

  const install = useCallback(async () => {
    if (!update || downloading) return;
    setDownloading(true);
    setError("");
    setProgress({ downloaded: 0 });

    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setProgress({ downloaded: 0, total: event.data.contentLength });
        } else if (event.event === "Progress") {
          setProgress((current) => ({
            ...current,
            downloaded: current.downloaded + event.data.chunkLength,
          }));
        }
      });
      await relaunch();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setDownloading(false);
    }
  }, [downloading, update]);

  const percent = progress.total
    ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
    : undefined;

  return (
    <Modal
      open={update !== null}
      title={downloading ? "正在更新 OfferFlow" : `发现新版本 v${update?.version ?? ""}`}
      okText={downloading ? "正在安装…" : "立即更新"}
      cancelText="稍后更新"
      okButtonProps={{ loading: downloading, disabled: downloading }}
      cancelButtonProps={{ disabled: downloading }}
      closable={!downloading}
      maskClosable={!downloading}
      keyboard={!downloading}
      onOk={() => void install()}
      onCancel={() => setUpdate(null)}
    >
      {downloading ? (
        <Progress percent={percent} status={error ? "exception" : "active"} />
      ) : (
        <>
          <Typography.Paragraph>
            当前版本将更新至 <Typography.Text strong>v{update?.version}</Typography.Text>。
            安装完成后应用会自动重启。
          </Typography.Paragraph>
          {renderedBody && (
            <div
              className="updater-release-notes"
              dangerouslySetInnerHTML={{ __html: renderedBody }}
            />
          )}
        </>
      )}
      {error && <Alert type="error" showIcon message="更新失败" description={error} />}
    </Modal>
  );
}
