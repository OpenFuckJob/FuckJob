import { useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Alert, Button, Image, Spin, Tabs, Typography } from "antd";
import "./App.css";
import Logo from "./assets/logo.png";
import { useAppConfig } from "@/hooks/useAppConfig";
import type { AppRuntimeConfig, BrowserConfig, GreetConfig, JobFilterConfig, RegexRule, ReplayConfig, ReplyResource, ReplyTemplate, ResumeConfig } from "@/types/app-config";
import { Onboarding } from "@/view/onboarding";
import { ConfigPage } from "@/view/config";
import ConversationDebugPage from "@/view/conversation-debug";
import JobDataPage from "@/view/job-data";
import ResumeOptimizerPage from "@/view/resume-optimizer";
import WorkspacePage from "@/view/workspace";
import AboutDataPage from "@/view/about-data";

type AppTabKey = "workspace" | "job-data" | "conversation-debug" | "resume-optimizer" | "config" | "about-data";
const tabs: Array<{ key: AppTabKey; label: string }> = [
  { key: "workspace", label: "工作台" }, { key: "job-data", label: "岗位管理" },
  { key: "config", label: "配置中心" },
  { key: "about-data", label: "关于与数据" },
];
const updateAt = <T,>(items: T[], index: number, next: Partial<T>) => items.map((item, i) => i === index ? { ...item, ...next } : item);

function MainShell({ config, update, save, status, message, dirty, importConfig, exportConfig }: {
  config: AppRuntimeConfig; update: (fn: (c: AppRuntimeConfig) => AppRuntimeConfig) => void;
  save: (next?: AppRuntimeConfig) => Promise<boolean>; status: "idle" | "loading" | "saved" | "error"; message: string; dirty: boolean;
  importConfig: (path: string) => Promise<void>; exportConfig: (path: string) => Promise<void>;
}) {
  const [activeTab, setActiveTab] = useState<AppTabKey>("workspace");
  const [configGroup, setConfigGroup] = useState<"resume" | "llm" | "job" | "greet" | "reply" | "browser">("resume");
  const navigate = async (next: AppTabKey) => {
    if (activeTab === "config" && next !== "config" && dirty) {
      const leave = await confirm("配置尚未保存，离开后本次修改仍会保留在当前界面，但任务不会使用这些修改。是否继续？", {
        title: "存在未保存配置",
        kind: "warning",
      });
      if (!leave) return;
    }
    setActiveTab(next);
  };
  const openConfig = (group: typeof configGroup) => { setConfigGroup(group); setActiveTab("config"); };
  const openLlm = () => openConfig("llm");
  const merge = <K extends keyof AppRuntimeConfig>(key: K, next: Partial<AppRuntimeConfig[K]>) => update((c) => ({ ...c, [key]: { ...(c[key] as object), ...next } }));
  const configPage = <ConfigPage config={config} status={status} message={message} dirty={dirty} initialGroup={configGroup}
    onOpenLlmConfig={openLlm}
    updateLlm={(llm_config) => update((c) => ({ ...c, llm_config }))}
    persistLlm={async (llm_config) => save({ ...config, llm_config })}
    updateJobFilter={(v: Partial<JobFilterConfig>) => merge("job_filter_config", v)}
    updateGreet={(v: Partial<GreetConfig>) => merge("greet_config", v)}
    updateGreetDefaultResource={(i: number, v: Partial<ReplyResource>) => update((c) => ({ ...c, greet_config: { ...c.greet_config, default_template: updateAt(c.greet_config.default_template, i, v) } }))}
    addGreetDefaultResource={() => update((c) => ({ ...c, greet_config: { ...c.greet_config, default_template: [...c.greet_config.default_template, { resource_type: "Text", content: "" }] } }))}
    removeGreetDefaultResource={(i: number) => update((c) => ({ ...c, greet_config: { ...c.greet_config, default_template: c.greet_config.default_template.filter((_, x) => x !== i) } }))}
    updateReplay={(v: Partial<ReplayConfig>) => merge("replay_config", v)}
    updateReplyTemplate={(i: number, v: Partial<ReplyTemplate>) => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: updateAt(c.replay_config.templates, i, v) } }))}
    addReplyTemplate={() => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: [...c.replay_config.templates, { regex_rule: { name: "", pattern: "", limit: 5 }, content: [{ resource_type: "Text", content: "" }] }] } }))}
    removeReplyTemplate={(i: number) => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: c.replay_config.templates.filter((_, x) => x !== i) } }))}
    updateReplyResource={(ti: number, ri: number, v: Partial<ReplyResource>) => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: c.replay_config.templates.map((t, i) => i === ti ? { ...t, content: updateAt(t.content, ri, v) } : t) } }))}
    addReplyResource={(ti: number) => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: c.replay_config.templates.map((t, i) => i === ti ? { ...t, content: [...t.content, { resource_type: "Text", content: "" }] } : t) } }))}
    removeReplyResource={(ti: number, ri: number) => update((c) => ({ ...c, replay_config: { ...c.replay_config, templates: c.replay_config.templates.map((t, i) => i === ti ? { ...t, content: t.content.filter((_, x) => x !== ri) } : t) } }))}
    updateBrowser={(v: Partial<BrowserConfig>) => merge("browser_config", v)} updateResume={(v: Partial<ResumeConfig>) => merge("resume_config", v)}
    updateRule={(i: number, v: Partial<RegexRule>) => update((c) => ({ ...c, job_filter_config: { ...c.job_filter_config, regex_rules: updateAt(c.job_filter_config.regex_rules, i, v) } }))}
    addRule={() => update((c) => ({ ...c, job_filter_config: { ...c.job_filter_config, regex_rules: [...c.job_filter_config.regex_rules, { name: "", pattern: "", target: "All", mode: "ACCEPT" }] } }))}
    removeRule={(i: number) => update((c) => ({ ...c, job_filter_config: { ...c.job_filter_config, regex_rules: c.job_filter_config.regex_rules.filter((_, x) => x !== i) } }))}
    saveConfig={() => { void save(); }} importConfig={importConfig} exportConfig={exportConfig} />;

  const content = activeTab === "workspace" ? <WorkspacePage onNavigate={(tab) => void navigate(tab)} onOpenConfig={openConfig} /> : activeTab === "job-data" ? <JobDataPage aiConfigured={!!config.llm_config} onConfigureAi={openLlm} /> : activeTab === "conversation-debug" ? <ConversationDebugPage aiConfigured={!!config.llm_config} onConfigureAi={openLlm} /> : activeTab === "resume-optimizer" ? <ResumeOptimizerPage config={config} onOpenResumeConfig={() => openConfig("resume")} onOpenLlmConfig={openLlm} updateResume={(v) => merge("resume_config", v)} /> : activeTab === "about-data" ? <AboutDataPage /> : configPage;
  return <main className="app-shell"><header className="app-header"><div className="app-brand"><div className="app-logo"><Image src={Logo} preview={false} /></div><div className="app-brand-info"><Typography.Title level={5} className="app-title">Fuck Job</Typography.Title><span className="app-brand-subtitle">本地优先的求职工具</span></div></div><Tabs activeKey={activeTab} className="app-tabs" items={tabs} onChange={(k) => void navigate(k as AppTabKey)} /></header><section className="app-content">{content}</section></main>;
}

export default function App() {
  const app = useAppConfig();
  if (app.status === "loading" && !app.config) return <main className="app-loading"><Spin size="large" tip="正在加载本地配置" /></main>;
  if (!app.config) return <main className="app-loading"><Alert type="error" showIcon message="无法加载本地配置" description={app.message} action={<Button onClick={() => void app.load()}>重试</Button>} /></main>;
  if (!app.config.onboarding_completed) return <Onboarding config={app.config} onFinish={app.save} />;
  return <MainShell config={app.config} update={app.updateConfig} save={app.save} status={app.status} message={app.message} dirty={app.dirty} importConfig={app.importConfig} exportConfig={app.exportConfig} />;
}
