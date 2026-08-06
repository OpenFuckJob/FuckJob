import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Alert, Modal, Progress, Typography, message } from "antd";
import MarkdownIt from "markdown-it";

let automaticCheckStarted = false;

type DownloadState = {
  downloaded: number;
  total?: number;
};

interface UpdaterContextType {
  checking: boolean;
  update: Update | null;
  downloading: boolean;
  progress: DownloadState;
  error: string;
  checkForUpdates: (manual?: boolean) => Promise<Update | null>;
  install: () => Promise<void>;
  dismissUpdate: () => void;
}

const UpdaterContext = createContext<UpdaterContextType | null>(null);

export function UpdaterProvider({ children }: { children: ReactNode }) {
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<DownloadState>({ downloaded: 0 });
  const [error, setError] = useState("");

  const checkForUpdates = useCallback(async (manual = false): Promise<Update | null> => {
    if (!isTauri()) {
      if (manual) {
        void message.info("当前处于 Web 开发模式，更新功能仅在桌面客户端可用");
      }
      return null;
    }

    setChecking(true);
    setError("");

    try {
      const availableUpdate = await check({ timeout: 15_000 });
      setUpdate(availableUpdate);
      if (manual) {
        if (availableUpdate) {
          void message.success(`发现新版本 v${availableUpdate.version}`);
        } else {
          void message.success("当前已是最新版本");
        }
      }
      return availableUpdate;
    } catch (reason) {
      const errMsg = reason instanceof Error ? reason.message : String(reason);
      console.warn("检查更新失败", reason);
      if (manual) {
        void message.error(`检查更新失败：${errMsg}`);
      }
      return null;
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    if (!isTauri() || automaticCheckStarted) return;
    automaticCheckStarted = true;

    void checkForUpdates(false);
  }, [checkForUpdates]);

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

  const dismissUpdate = useCallback(() => {
    if (!downloading) {
      setUpdate(null);
    }
  }, [downloading]);

  const value = useMemo(
    () => ({
      checking,
      update,
      downloading,
      progress,
      error,
      checkForUpdates,
      install,
      dismissUpdate,
    }),
    [checking, update, downloading, progress, error, checkForUpdates, install, dismissUpdate]
  );

  return <UpdaterContext.Provider value={value}>{children}</UpdaterContext.Provider>;
}

export function useUpdater() {
  const context = useContext(UpdaterContext);
  if (!context) {
    throw new Error("useUpdater 必须在 UpdaterProvider 内部使用");
  }
  return context;
}

export function AutoUpdaterModal() {
  const { update, downloading, progress, error, install, dismissUpdate } = useUpdater();

  const renderedBody = useMemo(() => {
    if (!update?.body) return "";
    const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
    return md.render(update.body);
  }, [update?.body]);

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
      onCancel={dismissUpdate}
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

export function AutoUpdater() {
  return null;
}
