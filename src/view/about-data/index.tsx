import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { Alert, Button, Card, Descriptions, Space, Typography, message } from "antd";
import {
  clearLogs, clearModelKey, exportDataBackup, getCredentialStatus, getDataDirectory,
  resetAppConfig, restoreDataBackup, type CredentialStatus,
} from "@/lib/dataManagement";

const REPOSITORY = "https://github.com/JWJW000/fuckJob_client";
const REPOSITORY_BRANCH = `${REPOSITORY}/blob/master`;

export default function AboutDataPage() {
  const [version, setVersion] = useState("0.1.1");
  const [directory, setDirectory] = useState("");
  const [credential, setCredential] = useState<CredentialStatus | null>(null);
  useEffect(() => {
    void getVersion().then(setVersion).catch(() => undefined);
    void getDataDirectory().then(setDirectory).catch(() => undefined);
    void getCredentialStatus().then(setCredential).catch(() => undefined);
  }, []);
  const run = async (operation: () => Promise<unknown>, success: string) => {
    try { await operation(); message.success(success); } catch (error) { message.error(error instanceof Error ? error.message : "操作失败"); }
  };
  const exportBackup = async () => {
    const path = await save({ title: "导出数据备份", defaultPath: `fuckjob-backup-${new Date().toISOString().slice(0, 10)}.zip`, filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (path) await run(() => exportDataBackup(path), "备份已导出");
  };
  const restoreBackup = async () => {
    const path = await open({ title: "选择数据备份", multiple: false, filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (typeof path !== "string" || !(await confirm("恢复会替换当前配置、岗位、聊天、分析和简历数据。是否继续？", { title: "确认替换本地数据", kind: "warning" }))) return;
    await run(async () => { const result = await restoreDataBackup(path); await message.info(result.message); }, "备份恢复完成");
  };
  const clearKey = async () => {
    if (!(await confirm("清除钥匙串中的模型密钥？环境变量密钥不会被删除。", { title: "清除模型密钥", kind: "warning" }))) return;
    await run(async () => setCredential(await clearModelKey()), "模型密钥已清除");
  };
  const sourceLabel = credential?.source === "environment" ? "环境变量（清除钥匙串后仍生效）" : credential?.source === "keychain" ? "系统钥匙串" : "未配置";
  return <Space orientation="vertical" size="middle" style={{ width: "100%", maxWidth: 920 }}>
    <div><Typography.Title level={2}>关于与数据</Typography.Title><Typography.Paragraph type="secondary">Fuck Job {version} · 本地优先的开源求职工具</Typography.Paragraph></div>
    <Alert type="info" showIcon title="隐私与网络边界" description="无需账号，不含遥测、自动更新或原项目服务器连接。配置、岗位、聊天、分析和简历保存在本机；仅在你使用模型功能时，所选岗位、简历或聊天上下文会发送给你配置的 LLM 服务；仅在启动自动化时访问 BOSS 直聘或猎聘。" />
    <Card title="开源许可"><Typography.Paragraph>本项目采用 Apache License 2.0。</Typography.Paragraph><Space wrap><Button onClick={() => void openUrl(`${REPOSITORY_BRANCH}/LICENSE`)}>查看完整许可</Button><Button onClick={() => void openUrl(`${REPOSITORY_BRANCH}/docs/privacy-and-network.md`)}>隐私与网络文档</Button><Button onClick={() => void openUrl(`${REPOSITORY_BRANCH}/docs/model-configuration.md`)}>模型配置文档</Button><Button onClick={() => void openUrl(REPOSITORY)}>打开代码仓库</Button></Space></Card>
    <Card title="本地数据"><Descriptions column={1} size="small"><Descriptions.Item label="数据目录">{directory || "正在读取…"}</Descriptions.Item><Descriptions.Item label="模型密钥来源">{sourceLabel}</Descriptions.Item></Descriptions><Space wrap>
      <Button onClick={() => void (directory && revealItemInDir(directory))}>在文件管理器中显示</Button>
      <Button onClick={() => void exportBackup()}>导出备份</Button><Button onClick={() => void restoreBackup()}>恢复备份</Button>
      <Button onClick={() => void (async () => { if (await confirm("清除本地运行日志？", { title: "清除日志", kind: "warning" })) await run(clearLogs, "日志已清除"); })()}>清除日志</Button>
      <Button onClick={() => void clearKey()}>清除模型密钥</Button>
      <Button danger onClick={() => void (async () => { if (await confirm("重置应用配置？岗位、聊天、简历数据和模型密钥会保留。", { title: "重置配置", kind: "warning" })) await run(resetAppConfig, "配置已重置，重新打开应用后生效"); })()}>重置应用配置</Button>
    </Space></Card>
  </Space>;
}
