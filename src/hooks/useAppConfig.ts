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
    const configAtSaveStart = config;
    setStatus("loading");
    try {
      // 用后端落盘后的那份而不是提交的那份：保存路径上会做迁移、夹取上下界、
      // 补生成拟人化的人格种子。拿旧值当已保存快照的话，下次保存又原样提交一遍，
      // 种子于是每存一次换一个，拟人化的「当天节奏稳定」就不成立了
      const saved = await saveAppConfig(value);
      savedSnapshot.current = JSON.stringify(saved);
      // 保存期间用户可能又改了别处，这时不能拿回包覆盖他正在编辑的内容
      setConfig((current) => current === configAtSaveStart ? saved : current);
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
