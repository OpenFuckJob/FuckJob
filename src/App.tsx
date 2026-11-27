import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Badge, Button, ConfigProvider, Spin, Tabs, Tooltip, Typography, theme as antdTheme } from "antd";
import { BellOutlined, DesktopOutlined, MoonOutlined, RocketOutlined, SunOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { useAppConfig } from "@/hooks/useAppConfig";
import { mergeUniqueKeywords } from "@/lib/blacklist-presets";
import { THEME_MODE_LABELS, useThemeMode, type ThemeMode } from "@/lib/theme";
import type { CommandResult } from "@/types/command";
import { countJobTasks, type JobTaskOverview } from "@/types/rpa";
import { copyJobProfile, DEFAULT_REGEX_RULE_LIMIT, getAnalysisConfig, getDefaultJobProfile, getHumanizeConfig, getJobProfiles, getNotificationConfig, getPeriodicDeliveryConfig, getPlatformFilterConfig, getReplyPollingConfig, isLlmActive, isLlmConfigured, selectProfileAfterRemoval, type AnalysisConfig, type AppRuntimeConfig, type BrowserConfig, type ConfigGroup, type GreetConfig, type GreetResource, type HumanizeConfig, type JobFilterConfig, type JobProfile, type NotificationConfig, type PeriodicDeliveryConfig, type PlatformFilterConfig, type RegexRule, type ReplayConfig, type ReplyPollingConfig, type ReplyResource, type ReplyTemplate, type ResumeConfig } from "@/types/app-config";
import type { JobDetail } from "@/types/job-detail";
import { Onboarding } from "@/view/onboarding";
import { ConfigPage } from "@/view/config";
import JobDataPage from "@/view/job-data";
import JobOverviewPage from "@/view/job-overview";
import PlaygroundPage from "@/view/playground";
import ResumeOptimizerPage from "@/view/resume-optimizer";
import WorkspacePage from "@/view/workspace";
import ManualReviewDrawer, { useManualReview } from "@/view/workspace/manual-review-drawer";
import { AutoUpdaterModal, UpdaterProvider } from "@/lib/updater";

type AppTabKey = "workspace" | "job-overview" | "job-data" | "resume-optimizer" | "config";
// 测试模式不在这里：它是调配置的工具，入口收在「配置中心 · 系统能力」下，
// 与大模型、浏览器环境并列——顶层这几个页签留给日常求职的主流程
const tabs: Array<{ key: AppTabKey; label: string }> = [
  { key: "workspace", label: "工作台" },
  { key: "job-overview", label: "求职数据" },
  { key: "job-data", label: "岗位管理" },
  { key: "resume-optimizer", label: "模拟面试" },
  { key: "config", label: "配置中心" },
];
const updateAt = <T,>(items: T[], index: number, next: Partial<T>) => items.map((item, i) => i === index ? { ...item, ...next } : item);
const moveAt = <T,>(items: T[], index: number, offset: -1 | 1) => {
  const target = index + offset;
  if (target < 0 || target >= items.length) return items;
  const next = [...items];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
};

function MainShell({ config, update, save, status, message, dirty, importConfig, exportConfig, themeMode, onCycleTheme }: {
  config: AppRuntimeConfig; update: (fn: (c: AppRuntimeConfig) => AppRuntimeConfig) => void;
  save: (next?: AppRuntimeConfig) => Promise<boolean>; status: "idle" | "loading" | "saved" | "error"; message: string; dirty: boolean;
  importConfig: (path: string) => Promise<void>; exportConfig: (path: string) => Promise<void>;
  themeMode: ThemeMode; onCycleTheme: () => void;
}) {
  const [activeTab, setActiveTab] = useState<AppTabKey>("workspace");
  const [focusJobId, setFocusJobId] = useState<string>();
  const [interviewJob, setInterviewJob] = useState<JobDetail>();
  const [configGroup, setConfigGroup] = useState<ConfigGroup>("job");
  const [activeProfileId, setActiveProfileId] = useState(() => getDefaultJobProfile(config).id);
  const [reviewDrawerOpen, setReviewDrawerOpen] = useState(false);
  const [headerTaskCounts, setHeaderTaskCounts] = useState({ active: 0, running: 0, queued: 0 });
  const manualReview = useManualReview();

  // 全局任务运行态感知：离开工作台时顶栏仍能看到后台任务是否运行中
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const res = await invoke<CommandResult<JobTaskOverview>>("get_job_task_status");
        if (!disposed && res.success && res.data) {
          setHeaderTaskCounts(countJobTasks(res.data.tasks));
        }
      } catch {
        // ignore transient poll errors
      }
      if (!disposed) timer = setTimeout(() => void poll(), 4000);
    };
    void poll();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, []);

  // 桌面快捷键：Cmd/Ctrl + 1~5 快速切换顶层页签
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
      const idx = Number(event.key) - 1;
      if (idx >= 0 && idx < tabs.length) {
        event.preventDefault();
        setActiveTab(tabs[idx].key);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
  const profiles = getJobProfiles(config);
  const llmConfigured = isLlmConfigured(config);
  const llmActive = isLlmActive(config);
  const activeProfile = profiles.find((profile) => profile.id === activeProfileId)
    ?? getDefaultJobProfile(config);
  const profileConfig = useMemo(() => ({
    ...config,
    job_profiles: profiles,
    default_job_profile_id: config.default_job_profile_id || getDefaultJobProfile(config).id,
    job_filter_config: activeProfile.job_filter_config,
    platform_filter_config: getPlatformFilterConfig(activeProfile),
    resume_config: activeProfile.resume_config,
    greet_config: activeProfile.greet_config,
    replay_config: activeProfile.replay_config,
    analysis_config: getAnalysisConfig(activeProfile),
  }), [activeProfile, config, profiles]);

  // 不完整的大模型服务会由 useAppConfig 标为校验错误，只留在内存中；用户补齐
  // 后状态恢复为 idle，下面的防抖保存会自动继续。API Key 有独立的凭据保存流程，
  // 获取模型列表不依赖把这份半成品配置落盘。
  useEffect(() => {
    if (!dirty || status === "loading" || status === "error") return;
    const timer = window.setTimeout(() => { void save(); }, 700);
    return () => window.clearTimeout(timer);
  }, [dirty, save, status]);

  const navigate = (next: AppTabKey) => setActiveTab(next);
  const openConversation = (jobId: string) => { setFocusJobId(jobId); setActiveTab("job-data"); };
  const clearFocusJob = useCallback(() => setFocusJobId(undefined), []);
  const startInterview = (job: JobDetail) => { setInterviewJob(job); setActiveTab("resume-optimizer"); };
  const clearInterviewJob = useCallback(() => setInterviewJob(undefined), []);
  const openConfig = (group: typeof configGroup) => { setConfigGroup(group); setActiveTab("config"); };
  const openLlm = () => openConfig("llm");
  const merge = <K extends keyof AppRuntimeConfig>(key: K, next: Partial<AppRuntimeConfig[K]>) => update((c) => ({ ...c, [key]: { ...(c[key] as object), ...next } }));
  const updateActiveProfile = (mutate: (profile: JobProfile) => JobProfile) => update((c) => {
    const currentProfiles = getJobProfiles(c);
    const selected = currentProfiles.find((profile) => profile.id === activeProfileId)
      ?? getDefaultJobProfile(c);
    const nextProfiles = currentProfiles.map((profile) => profile.id === selected.id ? mutate(profile) : profile);
    return {
      ...c,
      job_profiles: nextProfiles,
      default_job_profile_id: c.default_job_profile_id || selected.id,
    };
  });
  const updateProfileSection = <K extends "job_filter_config" | "platform_filter_config" | "greet_config" | "replay_config" | "resume_config">(key: K, next: Partial<JobProfile[K]>) =>
    updateActiveProfile((profile) => ({ ...profile, [key]: { ...profile[key], ...next } }));
  const updatePlatformFilter = (next: Partial<PlatformFilterConfig>) =>
    updateActiveProfile((profile) => ({
      ...profile,
      platform_filter_config: {
        ...getPlatformFilterConfig(profile),
        ...next,
      },
    }));
  // 分析配置在旧方案里可能整块缺失，先补默认值再合并
  const updateAnalysis = (next: Partial<AnalysisConfig>) =>
    updateActiveProfile((profile) => ({ ...profile, analysis_config: { ...getAnalysisConfig(profile), ...next } }));
  // 轮询节奏是顶层配置，但同样可能整块缺失，不能直接走 merge
  const updatePolling = (next: Partial<ReplyPollingConfig>) =>
    update((c) => ({ ...c, reply_polling_config: { ...getReplyPollingConfig(c), ...next } }));
  const updatePeriodicDelivery = (next: Partial<PeriodicDeliveryConfig>) =>
    update((c) => ({ ...c, periodic_delivery_config: { ...getPeriodicDeliveryConfig(c), ...next } }));
  // 拟人化同样是顶层配置，旧配置里整块缺失，不能直接走 merge
  const updateHumanize = (next: Partial<HumanizeConfig>) =>
    update((c) => ({ ...c, humanize_config: { ...getHumanizeConfig(c), ...next } }));
  const updateNotification = (next: Partial<NotificationConfig>) =>
    update((c) => ({ ...c, notification_config: { ...getNotificationConfig(c), ...next } }));
  const blacklistCompany = (companyName: string) => {
    const trimmed = companyName.trim();
    if (!trimmed) return;
    updateActiveProfile((profile) => ({
      ...profile,
      job_filter_config: {
        ...profile.job_filter_config,
        company_exclude_keywords: mergeUniqueKeywords(
          profile.job_filter_config.company_exclude_keywords,
          [trimmed],
        ),
      },
    }));
  };
  // 测试模式可以挑任意一个方案来跑，选中的不一定是配置页当前编辑的那个，
  // 所以写回提示词时只能按 id 定位，不能复用 updateActiveProfile
  const savePlaygroundPrompts = (id: string, prompts: { greet_prompt: string | null; reply_prompt: string | null; semantic_filter_intent: string | null }) =>
    update((c) => ({
      ...c,
      job_profiles: getJobProfiles(c).map((profile) => profile.id === id ? {
        ...profile,
        greet_config: { ...profile.greet_config, reply_prompt: prompts.greet_prompt },
        replay_config: { ...profile.replay_config, reply_prompt: prompts.reply_prompt },
        job_filter_config: { ...profile.job_filter_config, semantic_filter_intent: prompts.semantic_filter_intent },
      } : profile),
      default_job_profile_id: c.default_job_profile_id || getDefaultJobProfile(c).id,
    }));
  const updateProfiles = (nextProfiles: JobProfile[], defaultId = config.default_job_profile_id) => update((c) => ({
    ...c,
    job_profiles: nextProfiles,
    default_job_profile_id: defaultId || nextProfiles[0]?.id || "default",
  }));
  const createProfile = (source: JobProfile, suffix = "新方案", meta?: Pick<JobProfile, "name" | "description">) => {
    const id = globalThis.crypto?.randomUUID?.() ?? `profile-${Date.now()}`;
    const copied = copyJobProfile(source, id, suffix);
    const next = meta ? { ...copied, ...meta } : copied;
    updateProfiles([...profiles, next]);
    setActiveProfileId(id);
  };
  const playgroundPage = <PlaygroundPage config={config} llmConfigured={llmConfigured} llmActive={llmActive}
    onOpenLlmConfig={openLlm} onSavePrompts={savePlaygroundPrompts} />;
  const configPage = <ConfigPage config={profileConfig} status={status} message={message} dirty={dirty} initialGroup={configGroup}
    playgroundSlot={playgroundPage}
    activeProfileId={activeProfile.id}
    onSelectProfile={setActiveProfileId}
    onCreateProfile={(meta) => createProfile(getDefaultJobProfile(config), "新方案", meta)}
    onDuplicateProfile={() => createProfile(activeProfile, "副本")}
    onUpdateProfileMeta={(next) => updateActiveProfile((profile) => ({ ...profile, ...next }))}
    onSetDefaultProfile={() => updateProfiles(profiles, activeProfile.id)}
    onArchiveProfile={() => {
      updateProfiles(profiles.map((profile) => profile.id === activeProfile.id ? { ...profile, archived: true } : profile));
      setActiveProfileId(selectProfileAfterRemoval(profiles, activeProfile.id)?.id ?? getDefaultJobProfile(config).id);
    }}
    onDeleteProfile={() => {
      const next = profiles.filter((profile) => profile.id !== activeProfile.id);
      updateProfiles(next);
      setActiveProfileId(selectProfileAfterRemoval(profiles, activeProfile.id)?.id ?? "default");
    }}
    onOpenLlmConfig={openLlm}
    updateLlm={(llm_config) => update((c) => ({ ...c, llm_config }))}
    persistLlm={async (llm_config) => save({ ...config, llm_config })}
    updateLlmEnabled={(llm_enabled) => update((c) => ({ ...c, llm_enabled }))}
    updateLlmFallbacks={(llm_fallbacks) => update((c) => ({ ...c, llm_fallbacks }))}
    updateLlmRetryConfig={(llm_retry_config) => update((c) => ({ ...c, llm_retry_config }))}
    persistConfig={() => save()}
    updateJobFilter={(v: Partial<JobFilterConfig>) => updateProfileSection("job_filter_config", v)}
    updatePlatformFilter={updatePlatformFilter}
    updateGreet={(v: Partial<GreetConfig>) => updateProfileSection("greet_config", v)}
    updateGreetDefaultResource={(i: number, v: Partial<GreetResource>) => updateActiveProfile((p) => ({ ...p, greet_config: { ...p.greet_config, default_template: updateAt(p.greet_config.default_template, i, v) } }))}
    addGreetDefaultResource={(resourceType: GreetResource["resource_type"] = "Text") => updateActiveProfile((p) => ({ ...p, greet_config: { ...p.greet_config, default_template: [...p.greet_config.default_template, { resource_type: resourceType, content: "" }] } }))}
    moveGreetDefaultResource={(i: number, offset: -1 | 1) => updateActiveProfile((p) => ({ ...p, greet_config: { ...p.greet_config, default_template: moveAt(p.greet_config.default_template, i, offset) } }))}
    removeGreetDefaultResource={(i: number) => updateActiveProfile((p) => ({ ...p, greet_config: { ...p.greet_config, default_template: p.greet_config.default_template.filter((_, x) => x !== i) } }))}
    updateReplay={(v: Partial<ReplayConfig>) => updateProfileSection("replay_config", v)}
    updateReplyTemplate={(i: number, v: Partial<ReplyTemplate>) => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: updateAt(p.replay_config.templates, i, v) } }))}
    addReplyTemplate={() => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: [...p.replay_config.templates, { regex_rule: { name: "", pattern: "", limit: DEFAULT_REGEX_RULE_LIMIT }, content: [{ resource_type: "Text", content: "" }] }] } }))}
    removeReplyTemplate={(i: number) => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: p.replay_config.templates.filter((_, x) => x !== i) } }))}
    updateReplyResource={(ti: number, ri: number, v: Partial<ReplyResource>) => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: p.replay_config.templates.map((t, i) => i === ti ? { ...t, content: updateAt(t.content, ri, v) } : t) } }))}
    addReplyResource={(ti: number) => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: p.replay_config.templates.map((t, i) => i === ti ? { ...t, content: [...t.content, { resource_type: "Text", content: "" }] } : t) } }))}
    removeReplyResource={(ti: number, ri: number) => updateActiveProfile((p) => ({ ...p, replay_config: { ...p.replay_config, templates: p.replay_config.templates.map((t, i) => i === ti ? { ...t, content: t.content.filter((_, x) => x !== ri) } : t) } }))}
    updateAnalysis={updateAnalysis}
    updatePolling={updatePolling}
    updatePeriodicDelivery={updatePeriodicDelivery}
    updateHumanize={updateHumanize}
    updateNotification={updateNotification}
    updateBrowser={(v: Partial<BrowserConfig>) => merge("browser_config", v)} updateResume={(v: Partial<ResumeConfig>) => updateProfileSection("resume_config", v)}
    updateRule={(i: number, v: Partial<RegexRule>) => updateActiveProfile((p) => ({ ...p, job_filter_config: { ...p.job_filter_config, regex_rules: updateAt(p.job_filter_config.regex_rules, i, v) } }))}
    addRule={() => updateActiveProfile((p) => ({ ...p, job_filter_config: { ...p.job_filter_config, regex_rules: [...p.job_filter_config.regex_rules, { name: "", pattern: "", target: "All", mode: "ACCEPT" }] } }))}
    addRules={(rules: RegexRule[]) => updateActiveProfile((p) => ({ ...p, job_filter_config: { ...p.job_filter_config, regex_rules: [...p.job_filter_config.regex_rules, ...rules] } }))}
    removeRule={(i: number) => updateActiveProfile((p) => ({ ...p, job_filter_config: { ...p.job_filter_config, regex_rules: p.job_filter_config.regex_rules.filter((_, x) => x !== i) } }))}
    importConfig={importConfig} exportConfig={exportConfig} />;

  const content = activeTab === "workspace"
    ? <WorkspacePage config={config} onNavigate={(tab) => void navigate(tab)} onOpenConfig={openConfig} onOpenConversation={openConversation} pendingReviewCount={manualReview.records.length} onOpenManualReview={() => setReviewDrawerOpen(true)} />
    : activeTab === "job-overview" ? <JobOverviewPage onNavigate={(target) => target === "job-data" ? navigate("job-data") : openConfig(target)} onOpenConversation={openConversation} /> : activeTab === "job-data" ? <JobDataPage aiConfigured={llmActive} llmConfigured={llmConfigured} onConfigureAi={openLlm} focusJobId={focusJobId} onFocusHandled={clearFocusJob} highMatchScore={getAnalysisConfig(getDefaultJobProfile(config)).high_match_score} onStartInterview={startInterview} onBlacklistCompany={blacklistCompany} /> : activeTab === "resume-optimizer" ? <ResumeOptimizerPage config={profileConfig} llmConfigured={llmConfigured} onOpenLlmConfig={openLlm} onOpenResumeConfig={() => openConfig("resume")} pendingInterviewJob={interviewJob} onPendingInterviewHandled={clearInterviewJob} /> : configPage;

  const themeIcon = themeMode === "dark" ? <MoonOutlined /> : themeMode === "light" ? <SunOutlined /> : <DesktopOutlined />;

  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="app-brand">
          <div className="app-logo-badge">
            <RocketOutlined />
          </div>
          <div className="app-brand-info">
            <Typography.Title level={5} className="app-title">OfferFlow</Typography.Title>
            <span className="app-brand-subtitle">智聘助手</span>
          </div>
        </div>
        <Tabs activeKey={activeTab} className="app-tabs" items={tabs} onChange={(k) => void navigate(k as AppTabKey)} />
        <div className="app-header-actions">
          {headerTaskCounts.active > 0 && (
            <Tooltip title="点击返回工作台查看实时队列与日志">
              <button
                type="button"
                className="header-status-pill is-running"
                onClick={() => navigate("workspace")}
              >
                <span className="of-pulse-dot" />
                <span>
                  运行中 {headerTaskCounts.running}
                  {headerTaskCounts.queued > 0 ? ` · 排队 ${headerTaskCounts.queued}` : ""}
                </span>
              </button>
            </Tooltip>
          )}
          <Tooltip title={manualReview.records.length > 0 ? `${manualReview.records.length} 条会话等待你接手` : "待人工处理会话（当前清空）"}>
            <button
              type="button"
              className={`header-status-pill${manualReview.records.length > 0 ? " is-urgent" : ""}`}
              onClick={() => setReviewDrawerOpen(true)}
            >
              <Badge count={manualReview.records.length} size="small" offset={[2, -2]}>
                <BellOutlined style={{ fontSize: 14, color: "inherit" }} />
              </Badge>
              <span>待处理</span>
            </button>
          </Tooltip>
          <Tooltip title={`外观：${THEME_MODE_LABELS[themeMode]}（点击切换）`}>
            <Button
              type="text"
              size="small"
              icon={themeIcon}
              aria-label={`切换主题，当前为${THEME_MODE_LABELS[themeMode]}`}
              onClick={onCycleTheme}
            />
          </Tooltip>
        </div>
      </header>
      <section className={`app-content${activeTab === "config" ? " is-frameless" : ""}`}>{content}</section>
      <ManualReviewDrawer
        open={reviewDrawerOpen}
        onClose={() => setReviewDrawerOpen(false)}
        records={manualReview.records}
        loading={manualReview.loading}
        onResolve={(record) => void manualReview.resolve(record)}
        onClear={() => void manualReview.clear()}
        onOpenConversation={(jobId) => {
          setReviewDrawerOpen(false);
          openConversation(jobId);
        }}
      />
    </main>
  );
}

export default function App() {
  const app = useAppConfig();
  const { mode: themeMode, isDark, cycleMode } = useThemeMode();
  if (app.status === "loading" && !app.config) return <main className="app-loading"><Spin size="large" tip="正在加载本地配置" /></main>;
  if (!app.config) return <main className="app-loading"><Alert type="error" showIcon title="无法加载本地配置" description={app.message} action={<Button onClick={() => void app.load()}>重试</Button>} /></main>;

  return (
    <UpdaterProvider>
      <ConfigProvider
        theme={{
          algorithm: isDark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
          token: {
            colorPrimary: "#1677ff",
            borderRadius: 10,
            fontFamily: "-apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', Roboto, sans-serif",
            ...(isDark
              ? {
                  colorBgContainer: "#162033",
                  colorBgElevated: "#1b2740",
                  colorTextBase: "#f1f5f9",
                  colorTextSecondary: "#94a3b8",
                  colorBorder: "#263244",
                }
              : {
                  colorBgContainer: "#ffffff",
                  colorTextBase: "#0f172a",
                  colorTextSecondary: "#64748b",
                }),
          },
          components: {
            Card: {
              paddingLG: 20,
            },
            Button: {
              borderRadius: 8,
              fontWeight: 500,
            },
            Tabs: {
              cardBg: isDark ? "#0f172a" : "#f8fafc",
            },
            Slider: {
              railSize: 6,
              handleSize: 12,
              handleSizeHover: 14,
              handleLineWidth: 3,
              handleLineWidthHover: 4,
              railBg: isDark ? "#334155" : "#e2e8f0",
              railHoverBg: isDark ? "#475569" : "#cbd5e1",
              // 默认的已选轨道是浅蓝，混在灰轨道里几乎看不出滑到了哪
              trackBg: "#1677ff",
              trackHoverBg: "#0958d9",
            },
          },
        }}
      >
        <AutoUpdaterModal />
        {!app.config.onboarding_completed ? (
          <Onboarding config={app.config} onFinish={app.save} />
        ) : (
          <MainShell config={app.config} update={app.updateConfig} save={app.save} status={app.status} message={app.message} dirty={app.dirty} importConfig={app.importConfig} exportConfig={app.exportConfig} themeMode={themeMode} onCycleTheme={cycleMode} />
        )}
      </ConfigProvider>
    </UpdaterProvider>
  );
}
