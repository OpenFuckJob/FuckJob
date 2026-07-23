import { useCallback, useEffect, useRef, useState } from "react";
import { exportAppConfig, importAppConfig, loadAppConfig, saveAppConfig } from "@/lib/tauriConfig";
import type { AppRuntimeConfig, StatusKind } from "@/types/app-config";

const errorMessage = (error: unknown) => error instanceof Error ? error.message : "操作失败";

export function useAppConfig() {
  const [config, setConfig] = useState<AppRuntimeConfig | null>(null);
  const [status, setStatus] = useState<StatusKind>("loading");
  const [message, setMessage] = useState("");
  const savedSnapshot = useRef("");
  const dirty = Boolean(config) && JSON.stringify(config) !== savedSnapshot.current;

  const load = useCallback(async () => {
    setStatus("loading");
    try {
      const value = await loadAppConfig();
      savedSnapshot.current = JSON.stringify(value);
      setConfig(value);
      setStatus("idle");
      setMessage("");
    } catch (error) {
      setStatus("error");
      setMessage(errorMessage(error));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const updateConfig = useCallback((update: (current: AppRuntimeConfig) => AppRuntimeConfig) => {
    setConfig((current) => current ? update(current) : current);
    setStatus((current) => current === "loading" ? current : "idle");
    setMessage("");
  }, []);

  const save = useCallback(async (nextConfig?: AppRuntimeConfig) => {
    const value = nextConfig ?? config;
    if (!value) return false;
    setStatus("loading");
    try {
      await saveAppConfig(value);
      const savedValue = JSON.stringify(value);
      savedSnapshot.current = savedValue;
      setConfig((current) => current && JSON.stringify(current) !== savedValue ? current : value);
      setStatus("saved");
      setMessage("配置已保存");
      return true;
    } catch (error) {
      setStatus("error");
      setMessage(errorMessage(error));
      return false;
    }
  }, [config]);

  const importConfig = useCallback(async (path: string) => {
    setStatus("loading");
    try {
      const value = await importAppConfig(path);
      savedSnapshot.current = JSON.stringify(value);
      setConfig(value);
      setStatus("saved");
      setMessage("配置已导入");
    } catch (error) {
      setStatus("error"); setMessage(errorMessage(error));
    }
  }, []);

  const exportConfig = useCallback(async (path: string) => {
    if (!config) return;
    setStatus("loading");
    try {
      await exportAppConfig(path, config);
      setStatus("saved"); setMessage("配置已导出");
    } catch (error) {
      setStatus("error"); setMessage(errorMessage(error));
    }
  }, [config]);

  return { config, status, message, dirty, load, save, importConfig, exportConfig, updateConfig };
}
