import {
  Form,
  Input,
  Select,
  Switch,
  Button,
  Card,
  Menu,
  Typography,
  Space,
  Row,
  Col,
  Divider,
  Empty,
  Cascader,
  Collapse,
  Alert,
  Dropdown,
  Modal,
  Segmented,
  Tabs,
  Tag,
} from "antd";
import {
  AppRuntimeConfig,
  ConfigGroup,
  StatusKind,
  MatchTarget,
  RuleMode,
  JobFilterConfig,
  PlatformFilterConfig,
  GreetConfig,
  GreetResource,
  ReplayConfig,
  ReplyResource,
  ReplyTemplate,
  BrowserConfig,
  ResumeConfig,
  RegexRule,
  JobProfile,
  AnalysisConfig,
  AnalysisTrigger,
  ReplyPollingConfig,
  PeriodicDeliveryConfig,
  HumanizeConfig,
  NotificationConfig,
  DEFAULT_PERIODIC_DELIVERY_CONFIG,
  getAnalysisConfig,
  getJobProfiles,
  getReplyPollingConfig,
  getPeriodicDeliveryConfig,
  getHumanizeConfig,
  getNotificationConfig,
  isLlmActive,
  isLlmConfigured,
  DEFAULT_AUTO_REPLY_WINDOW_HOURS,
  DEFAULT_MAX_AUTO_REPLIES,
  DEFAULT_MAX_REPLY_CHARS,
  DEFAULT_REGEX_RULE_LIMIT,
  DEFAULT_MAX_PARALLEL_TASKS,
  DEFAULT_HIGH_MATCH_SCORE,
  MIN_HIGH_MATCH_SCORE,
  DEFAULT_MAX_ANALYSIS_PER_TASK,
  MAX_ANALYSIS_PER_TASK,
} from "@/types/app-config";
import { NumberField } from "@/components/NumberField";
import {
  SettingGroup,
  SettingSlider,
  SettingToggle,
} from "@/components/SettingField";
import { RadioCardGroup } from "@/components/RadioCardGroup";
import ReplyPollingSection from "./ReplyPollingSection";
import PeriodicDeliverySection from "./PeriodicDeliverySection";
import HumanizeSection from "./HumanizeSection";
import NotificationSection from "./NotificationSection";
import Job51FilterSection from "./Job51FilterSection";
import { BLACKLIST_PRESETS, mergeUniqueKeywords } from "@/lib/blacklist-presets";
import {
  jobTypeOptions,
  salaryOptions,
  experienceOptions,
  degreeOptions,
  stageOptions,
  scaleOptions,
  industryTreeOptions,
  nationwideCityCode,
  cityTreeOptions,
} from "@/lib/constants";
import {
  ProductOutlined,
  CommentOutlined,
  GlobalOutlined,
  PlusOutlined,
  DeleteOutlined,
  LoadingOutlined,
  ControlOutlined,
  FilePdfOutlined,
  UploadOutlined,
  DownloadOutlined,
  CheckCircleOutlined,
  WarningOutlined,
  InfoCircleOutlined,
  RobotOutlined,
  DatabaseOutlined,
  ArrowUpOutlined,
  ArrowDownOutlined,
  EyeOutlined,
  EyeInvisibleOutlined,
  CopyOutlined,
  MoreOutlined,
  EditOutlined,
  StarOutlined,
  InboxOutlined,
  ThunderboltOutlined,
  ForwardOutlined,
  ProfileOutlined,
  RiseOutlined,
  SolutionOutlined,
  ClockCircleOutlined,
  FontSizeOutlined,
  SafetyOutlined,
  SlidersOutlined,
  StopOutlined,
  DollarOutlined,
} from "@ant-design/icons";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commandErrorMessage, type CommandResult } from "@/types/command";
import type { BrowserEnvStatus } from "@/types/rpa";
import { LlmConfigPanel } from "./LlmConfigPanel";
import { DataManagementPanel } from "./DataManagementPanel";
import { AboutPanel } from "./AboutPanel";
import { BasicSettingsView } from "./BasicSettingsView";
import { AiFeatureGate } from "@/components/AiFeatureGate";

const { Title, Text } = Typography;

/**
 * 自动回复的策略。
 *
 * 后端是 enable_llm / enable_template_reply 两个独立 bool，界面上收敛成一次单选：
 * 两个平级开关会让人以为「都打开」是某种冲突配置，实际它是最常用的那一档。
 */
type ReplyStrategy = "off" | "template" | "llm" | "template_first";
type ParsedKeywordFilters = Pick<
  JobFilterConfig,
  | "keywords"
  | "exclude_keywords"
  | "company_keywords"
  | "company_exclude_keywords"
>;

type ReplyStrategyFlags = Pick<
  ReplayConfig,
  "enable_llm" | "enable_template_reply"
>;

const REPLY_STRATEGY_FLAGS: Record<ReplyStrategy, ReplyStrategyFlags> = {
  off: { enable_llm: false, enable_template_reply: false },
  template: { enable_llm: false, enable_template_reply: true },
  llm: { enable_llm: true, enable_template_reply: false },
  template_first: { enable_llm: true, enable_template_reply: true },
};

const REPLY_STRATEGY_OPTIONS: {
  value: ReplyStrategy;
  label: string;
  description: string;
  needsLlm: boolean;
  recommended?: boolean;
}[] = [
  {
    value: "template_first",
    label: "规则优先，AI 兜底",
    description: "命中规则的直接发话术，不耗额度；其余交给 AI",
    needsLlm: true,
    recommended: true,
  },
  {
    value: "llm",
    label: "仅 AI 回复",
    description: "每条未读都交给大模型判断怎么回",
    needsLlm: true,
  },
  {
    value: "template",
    label: "仅规则回复",
    description: "只回命中规则的消息，其余留给人工",
    needsLlm: false,
  },
  {
    value: "off",
    label: "关闭自动回复",
    description: "只同步消息，不代你发送任何内容",
    needsLlm: false,
  },
];

function replyStrategyOf(config: ReplayConfig): ReplyStrategy {
  if (config.enable_llm && config.enable_template_reply)
    return "template_first";
  if (config.enable_llm) return "llm";
  if (config.enable_template_reply) return "template";
  return "off";
}

const basePromptVariableItems = [
  {
    token: "{{job_content}}",
    description: "当前职位信息，包含标题、公司和岗位详情",
  },
];

const resumeContextVariableItem = {
  token: "{{resume_context}}",
  description: "简历解析内容，仅在简历配置启用注入时填充",
};

const buildCommonPromptVariables = (resumeEnabled: boolean) =>
  resumeEnabled
    ? [...basePromptVariableItems, resumeContextVariableItem]
    : basePromptVariableItems;

const buildReplyPromptVariables = (resumeEnabled: boolean) => [
  {
    token: "{{job_content}}",
    description: "职位摘要（岗位标题｜公司名｜薪资）",
  },
  {
    token: "{{job_description}}",
    description: "清洗后的岗位 JD 正文详情",
  },
  ...(resumeEnabled
    ? [
        {
          token: "{{resume_context}}",
          description: "简历解析内容（也可写作 {{resume}}），需开启简历注入",
        },
      ]
    : []),
  {
    token: "{{background_context}}",
    description: "背景上下文，补充简历里没有体现的个人诉求",
  },
  {
    token: "{{chat_history}}",
    description: "完整岗位沟通上下文记录",
  },
  {
    token: "{{message_content}}",
    description: "招聘方最新发来的一条消息内容",
  },
];

const normalizeMultiSelectCodes = (values: number[]) => {
  if (values.includes(0)) return [];
  return Array.from(new Set(values.filter((value) => value !== 0)));
};

const bossActiveThresholdOptions: Array<{
  value: PlatformFilterConfig["boss"]["active_threshold"];
  label: string;
}> = [
  { value: "online", label: "当前在线" },
  { value: "just_active", label: "刚刚活跃" },
  { value: "three_days", label: "3日内" },
  { value: "this_week", label: "本周内" },
  { value: "two_weeks", label: "2周内" },
  { value: "this_month", label: "本月内" },
  { value: "three_months", label: "3月内" },
  { value: "half_year", label: "半年内" },
];

/** 将历史配置中重复/细碎的活跃度档位平滑映射到精简后的 8 档 */
function normalizeBossActiveThreshold(
  value: PlatformFilterConfig["boss"]["active_threshold"],
): PlatformFilterConfig["boss"]["active_threshold"] {
  switch (value) {
    case "seven_days":
    case "seven_days_text":
    case "disabled":
      return "this_week";
    case "two_months":
      return "three_months";
    case "four_months":
    case "five_months":
    case "half_year_ago":
      return "half_year";
    default:
      return value;
  }
}

interface BossActiveThresholdScaleProps {
  value: PlatformFilterConfig["boss"]["active_threshold"];
  onChange: (value: PlatformFilterConfig["boss"]["active_threshold"]) => void;
}

function BossActiveThresholdScale({
  value,
  onChange,
}: BossActiveThresholdScaleProps) {
  const normalized = normalizeBossActiveThreshold(value);
  const selectedIndex = Math.max(
    0,
    bossActiveThresholdOptions.findIndex((option) => option.value === normalized),
  );

  return (
    <div className="rounded-lg border border-[var(--of-border)] bg-[var(--of-surface)] px-4 py-4">
      <div className="mb-3 flex items-center justify-between gap-3 text-xs text-[var(--of-text-secondary)]">
        <span>回复率更高（更活跃）</span>
        <span>覆盖更多岗位（更宽松）</span>
      </div>
      <div className="relative px-2 pt-1">
        <div className="absolute left-6 right-6 top-[13px] h-1 rounded-full bg-[var(--of-border-strong)]" />
        <div
          className="absolute left-6 top-[13px] h-1 rounded-full bg-blue-500 transition-all"
          style={{
            width:
              bossActiveThresholdOptions.length > 1
                ? `calc((100% - 3rem) * ${selectedIndex / (bossActiveThresholdOptions.length - 1)})`
                : 0,
          }}
        />
        <div className="relative z-10 grid grid-cols-4 gap-y-3 sm:grid-cols-8">
          {bossActiveThresholdOptions.map((option, index) => {
            const selected = option.value === normalized;
            const beforeSelected = index <= selectedIndex;
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={selected}
                className="group flex flex-col items-center gap-2 border-0 bg-transparent p-0 text-center outline-none cursor-pointer"
                onClick={() => onChange(option.value)}
              >
                <span
                  className={[
                    "grid h-6 w-6 place-items-center rounded-full border-2 bg-[var(--of-surface)] transition",
                    selected
                      ? "border-blue-500 shadow-[0_0_0_5px_rgba(59,130,246,0.14)]"
                      : beforeSelected
                        ? "border-blue-400"
                        : "border-[var(--of-border-strong)] group-hover:border-blue-300",
                  ].join(" ")}
                >
                  <span
                    className={[
                      "h-2.5 w-2.5 rounded-full transition",
                      selected
                        ? "bg-blue-600"
                        : beforeSelected
                          ? "bg-blue-300"
                          : "bg-slate-200 group-hover:bg-blue-200",
                    ].join(" ")}
                  />
                </span>
                <span
                  className={[
                    "px-1 text-xs font-medium leading-5 transition",
                    selected
                      ? "text-blue-600"
                      : beforeSelected
                        ? "text-slate-700"
                        : "text-slate-500 group-hover:text-slate-700",
                  ].join(" ")}
                >
                  {option.label}
                </span>
              </button>
            );
          })}
        </div>
      </div>
      <div className="mt-3 text-xs leading-5 text-slate-500">
        当前阈值：只处理活跃度不低于
        <Text className="mx-1" strong>
          {bossActiveThresholdOptions[selectedIndex]?.label ?? "本周内"}
        </Text>
        的招聘者，右侧更久未登录的岗位会直接跳过。
      </div>
    </div>
  );
}

interface PromptVariableGuideProps {
  items: typeof basePromptVariableItems;
}

function PromptVariableGuide({ items }: PromptVariableGuideProps) {
  return (
    <div className="mt-3 rounded-2xl border border-[var(--of-border)] bg-[var(--of-accent-soft)] p-4">
      <Text className="mb-3 block text-[10px] font-black uppercase tracking-[0.22em] text-[var(--of-accent)]">
        可用变量
      </Text>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        {items.map((item) => (
          <div
            key={item.token}
            className="rounded-xl border border-[var(--of-border)] bg-[var(--of-surface)] px-3 py-2"
          >
            <code className="font-mono text-[11px] font-bold text-emerald-500">
              {item.token}
            </code>
            <Text className="mt-1 block text-[11px] leading-5 text-[var(--of-text-secondary)]">
              {item.description}
            </Text>
          </div>
        ))}
      </div>
    </div>
  );
}

export type ConfigViewMode = "basic" | "advanced";

export interface ConfigPageProps {
  config: AppRuntimeConfig;
  status: StatusKind;
  message: string;
  dirty?: boolean;
  initialGroup?: ConfigGroup;
  initialMode?: ConfigViewMode;
  navigationSeq?: number;
  onOpenLlmConfig: () => void;
  updateLlm: (next: AppRuntimeConfig["llm_config"]) => void;
  persistLlm: (next: AppRuntimeConfig["llm_config"]) => Promise<boolean>;
  updateLlmEnabled: (next: AppRuntimeConfig["llm_enabled"]) => void;
  updateLlmFallbacks: (next: AppRuntimeConfig["llm_fallbacks"]) => void;
  updateLlmRetryConfig: (next: AppRuntimeConfig["llm_retry_config"]) => void;
  persistConfig: () => Promise<boolean>;
  updateJobFilter: (next: Partial<JobFilterConfig>) => void;
  updatePlatformFilter: (next: Partial<PlatformFilterConfig>) => void;
  updateGreet: (next: Partial<GreetConfig>) => void;
  updateGreetDefaultResource: (
    resourceIndex: number,
    next: Partial<GreetResource>,
  ) => void;
  addGreetDefaultResource: (
    resourceType?: GreetResource["resource_type"],
    content?: string,
  ) => void;
  moveGreetDefaultResource: (resourceIndex: number, offset: -1 | 1) => void;
  removeGreetDefaultResource: (resourceIndex: number) => void;
  updateReplay: (next: Partial<ReplayConfig>) => void;
  updateReplyTemplate: (index: number, next: Partial<ReplyTemplate>) => void;
  addReplyTemplate: () => void;
  removeReplyTemplate: (index: number) => void;
  updateReplyResource: (
    templateIndex: number,
    resourceIndex: number,
    next: Partial<ReplyResource>,
  ) => void;
  addReplyResource: (templateIndex: number) => void;
  removeReplyResource: (templateIndex: number, resourceIndex: number) => void;
  updateAnalysis: (next: Partial<AnalysisConfig>) => void;
  updatePolling: (next: Partial<ReplyPollingConfig>) => void;
  updatePeriodicDelivery: (next: Partial<PeriodicDeliveryConfig>) => void;
  updateHumanize: (next: Partial<HumanizeConfig>) => void;
  updateNotification: (next: Partial<NotificationConfig>) => void;
  updateBrowser: (next: Partial<BrowserConfig>) => void;
  updateResume: (next: Partial<ResumeConfig>) => void;
  updateRule: (index: number, next: Partial<RegexRule>) => void;
  addRule: () => void;
  addRules: (rules: RegexRule[]) => void;
  removeRule: (index: number) => void;
  importConfig: (path: string) => Promise<void>;
  exportConfig: (path: string) => Promise<void>;
  onConfigImported?: () => Promise<void>;
  activeProfileId: string;
  onSelectProfile: (id: string) => void;
  onCreateProfile: (meta: Pick<JobProfile, "name" | "description">) => void;
  onDuplicateProfile: () => void;
  onUpdateProfileMeta: (
    next: Partial<Pick<JobProfile, "name" | "description">>,
  ) => void;
  onSetDefaultProfile: () => void;
  onArchiveProfile: () => void;
  onDeleteProfile: () => void;
}

/** 自动分析的触发时机，一次只生效一个 */
const ANALYSIS_TRIGGER_OPTIONS: Array<{
  value: AnalysisTrigger;
  label: string;
  description: string;
  needsLlm: boolean;
  recommended?: boolean;
}> = [
  {
    value: "off",
    label: "关闭自动分析",
    description: "只在岗位详情页手动触发",
    needsLlm: false,
  },
  {
    value: "greet_sent",
    label: "打招呼发送成功后",
    description: "只分析投出去的岗位，最省额度",
    needsLlm: true,
    recommended: true,
  },
  {
    value: "filter_passed",
    label: "通过筛选规则后",
    description: "规则命中即分析，覆盖最全但更费额度",
    needsLlm: true,
  },
  {
    value: "reply_received",
    label: "收到 HR 回复后",
    description: "对方回复后才分析，最贴合面试准备",
    needsLlm: true,
  },
];

const configGroupKeys = [
  "runtime",
  "browser",
  "llm",
  "job",
  "resume",
  "greet",
  "reply",
  "analysis",
  "data",
  "about",
] as const;
type VisibleConfigGroup = (typeof configGroupKeys)[number];

const BASIC_GROUPS: ReadonlySet<ConfigGroup> = new Set([
  "job",
  "resume",
  "greet",
  "runtime",
  "llm",
]);

function resolveBasicAnchor(group?: ConfigGroup): string | null {
  switch (group) {
    case "job":
      return "basic-section-job";
    case "resume":
    case "greet":
      return "basic-section-resume";
    case "runtime":
      return "basic-section-runtime";
    case "llm":
      return "basic-section-llm";
    default:
      return null;
  }
}

const toVisibleConfigGroup = (group?: ConfigGroup): VisibleConfigGroup =>
  configGroupKeys.includes(group as VisibleConfigGroup)
    ? (group as VisibleConfigGroup)
    : "job";

const menuItems = [
  {
    type: "group" as const,
    label: "求职策略（高级）",
    children: [
      { key: "profile", icon: <ProductOutlined />, label: "多方案与精细筛选" },
    ],
  },
  {
    type: "group" as const,
    label: "系统能力",
    children: [
      { key: "runtime", icon: <SafetyOutlined />, label: "运行、防封与通知" },
      { key: "llm", icon: <RobotOutlined />, label: "AI 降级链与重试" },
      { key: "browser", icon: <GlobalOutlined />, label: "浏览器环境" },
    ],
  },
  {
    type: "group" as const,
    label: "数据与应用",
    children: [
      { key: "data", icon: <DatabaseOutlined />, label: "备份与设备共享" },
      { key: "about", icon: <InfoCircleOutlined />, label: "软件与更新" },
    ],
  },
];

const profileGroupKeys = [
  "job",
  "resume",
  "greet",
  "reply",
  "analysis",
] as const;
type ProfileGroup = (typeof profileGroupKeys)[number];
const isProfileGroup = (group: VisibleConfigGroup): group is ProfileGroup =>
  profileGroupKeys.includes(group as ProfileGroup);

const profileTabItems = [
  { key: "job", icon: <ProductOutlined />, label: "岗位筛选" },
  { key: "resume", icon: <FilePdfOutlined />, label: "简历配置" },
  { key: "greet", icon: <CommentOutlined />, label: "打招呼" },
  { key: "reply", icon: <CommentOutlined />, label: "自动回复" },
  { key: "analysis", icon: <ThunderboltOutlined />, label: "岗位分析" },
];

export function ConfigPage(props: ConfigPageProps) {
  const [viewMode, setViewMode] = useState<ConfigViewMode>(() => {
    if (props.initialMode) return props.initialMode;
    if (props.initialGroup && !BASIC_GROUPS.has(props.initialGroup)) return "advanced";
    return "basic";
  });
  const [activeGroup, setActiveGroup] = useState<VisibleConfigGroup>(() =>
    toVisibleConfigGroup(props.initialGroup),
  );
  const [jobFilterTab, setJobFilterTab] = useState<"basic" | "guard" | "platform">("basic");
  const [form] = Form.useForm();
  const [noticeModal, noticeModalContextHolder] = Modal.useModal();
  const [browserEnvStatus, setBrowserEnvStatus] =
    useState<BrowserEnvStatus | null>(null);
  const [ruleRequirement, setRuleRequirement] = useState("");
  const [generatingRules, setGeneratingRules] = useState(false);
  const [keywordRequirement, setKeywordRequirement] = useState("");
  const [parsingKeywords, setParsingKeywords] = useState(false);
  const [profileModalMode, setProfileModalMode] = useState<
    "create" | "edit" | null
  >(null);
  const [profileDraft, setProfileDraft] = useState({
    name: "",
    description: "",
  });
  const profiles = getJobProfiles(props.config);
  const activeProfile =
    profiles.find((profile) => profile.id === props.activeProfileId) ??
    profiles[0];
  const defaultProfileId =
    props.config.default_job_profile_id || profiles[0]?.id;
  const canArchive =
    activeProfile.id !== defaultProfileId && !activeProfile.archived;
  const canDelete =
    profiles.length > 1 && activeProfile.id !== defaultProfileId;
  const llmConfigured = isLlmConfigured(props.config);
  const llmActive = isLlmActive(props.config);
  // 「是否启用」与「当前是否填完整可调用」是两件事。模型名被清空时仍要保持
  // 编辑区展开，用户才能补回模型名或放弃这份配置。
  const llmEnabled = props.config.llm_enabled !== false;

  const showNotice = (
    type: "success" | "error" | "info" | "warning",
    title: string,
    content: string,
    width = 520,
  ) => {
    noticeModal[type]({
      title,
      content,
      centered: true,
      width,
      okText: "知道了",
    });
  };

  const openProfileModal = (mode: "create" | "edit") => {
    setProfileModalMode(mode);
    setProfileDraft(
      mode === "edit"
        ? {
            name: activeProfile.name,
            description: activeProfile.description ?? "",
          }
        : { name: "", description: "" },
    );
  };

  const saveProfileMeta = () => {
    const name = profileDraft.name.trim();
    if (!name) {
      showNotice("warning", "方案名称不能为空", "请输入方案名称后再继续。");
      return;
    }
    const meta = { name, description: profileDraft.description.trim() || null };
    if (profileModalMode === "create") props.onCreateProfile(meta);
    if (profileModalMode === "edit") props.onUpdateProfileMeta(meta);
    setProfileModalMode(null);
  };

  const handleProfileAction = ({ key }: { key: string }) => {
    if (key === "edit") openProfileModal("edit");
    if (key === "duplicate") props.onDuplicateProfile();
    if (key === "default") props.onSetDefaultProfile();
    if (key === "archive") {
      Modal.confirm({
        title: "归档这张求职方案？",
        content:
          "归档后不会出现在新任务的方案列表中，历史任务和会话仍可继续使用。",
        okText: "确认归档",
        cancelText: "取消",
        onOk: props.onArchiveProfile,
      });
    }
    if (key === "delete") {
      Modal.confirm({
        title: "删除这张求职方案？",
        content: "删除后无法恢复；已入队任务仍会使用原有执行快照。",
        okText: "删除",
        cancelText: "取消",
        okButtonProps: { danger: true },
        onOk: props.onDeleteProfile,
      });
    }
  };

  useEffect(() => {
    const nextGroup = toVisibleConfigGroup(props.initialGroup);
    setActiveGroup(nextGroup);
    const nextMode: ConfigViewMode = props.initialMode
      ? props.initialMode
      : props.initialGroup && !BASIC_GROUPS.has(props.initialGroup)
        ? "advanced"
        : "basic";
    setViewMode(nextMode);
    if (nextMode === "basic") {
      const anchor = resolveBasicAnchor(props.initialGroup);
      if (anchor) {
        window.setTimeout(() => {
          document.getElementById(anchor)?.scrollIntoView({ behavior: "smooth", block: "start" });
        }, 50);
      }
    }
  }, [props.initialGroup, props.initialMode, props.navigationSeq]);

  useEffect(() => {
    invoke<CommandResult<BrowserEnvStatus>>("check_browser_env")
      .then((result) => {
        if (result.success && result.data) {
          setBrowserEnvStatus(result.data);
        }
      })
      .catch(() => {});
  }, [props.config.browser_config]);

  const generateJobFilterRules = async () => {
    const requirement = ruleRequirement.trim();
    if (!requirement) {
      showNotice(
        "warning",
        "请先描述岗位筛选需求",
        "先输入岗位筛选需求，再生成高级规则。",
      );
      return;
    }
    if (!llmActive) {
      props.onOpenLlmConfig();
      return;
    }

    setGeneratingRules(true);
    try {
      const result = await invoke<CommandResult<RegexRule[]>>(
        "generate_job_filter_rules",
        { requirement },
      );
      if (!result.success || !result.data) {
        throw new Error(commandErrorMessage(result.error, "生成高级规则失败"));
      }
      props.addRules(result.data);
      setRuleRequirement("");
      showNotice(
        "success",
        "规则已生成",
        `已生成 ${result.data.length} 条规则，请检查后保存。`,
      );
    } catch (error) {
      showNotice(
        "error",
        "生成高级规则失败",
        error instanceof Error ? error.message : "生成高级规则失败",
      );
    } finally {
      setGeneratingRules(false);
    }
  };

  const parseJobKeywordFilters = async () => {
    const requirement = keywordRequirement.trim();
    if (!requirement) {
      showNotice("warning", "请先描述关键词需求", "输入需求后再提取筛选关键词。");
      return;
    }
    if (!llmActive) {
      props.onOpenLlmConfig();
      return;
    }

    setParsingKeywords(true);
    try {
      const result = await invoke<CommandResult<ParsedKeywordFilters>>(
        "parse_job_keyword_filters",
        { requirement },
      );
      if (!result.success || !result.data) {
        throw new Error(commandErrorMessage(result.error, "提取筛选关键词失败"));
      }
      const current = props.config.job_filter_config;
      props.updateJobFilter({
        keywords: mergeUniqueKeywords(current.keywords, result.data.keywords),
        exclude_keywords: mergeUniqueKeywords(
          current.exclude_keywords,
          result.data.exclude_keywords,
        ),
        company_keywords: mergeUniqueKeywords(
          current.company_keywords,
          result.data.company_keywords,
        ),
        company_exclude_keywords: mergeUniqueKeywords(
          current.company_exclude_keywords,
          result.data.company_exclude_keywords,
        ),
      });
      setKeywordRequirement("");
      showNotice(
        "success",
        "关键词已增量合并",
        "新提取的筛选词已去重合并到现有列表中，不会覆盖你原有的配置。",
      );
    } catch (error) {
      showNotice(
        "error",
        "提取筛选关键词失败",
        error instanceof Error ? error.message : "提取筛选关键词失败",
      );
    } finally {
      setParsingKeywords(false);
    }
  };

  const selectConfigFile = async () => {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "配置文件",
          extensions: ["yaml", "yml", "json"],
        },
      ],
    });

    if (typeof selected === "string") {
      await props.importConfig(selected);
    }
  };

  const selectExportConfigFile = async () => {
    const selected = await save({
      defaultPath: "app_config.yaml",
      filters: [
        {
          name: "配置文件",
          extensions: ["yaml", "yml"],
        },
      ],
    });

    if (typeof selected === "string") {
      await props.exportConfig(selected);
    }
  };

  const selectUserDataDir = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      form.setFieldValue(["browser_config", "user_data_dir"], selected);
      props.updateBrowser({ user_data_dir: selected });
    }
  };

  const selectChromeExePath = async () => {
    const selected = await open({ directory: false, multiple: false });
    if (typeof selected === "string") {
      form.setFieldValue(["browser_config", "chrome_exe_path"], selected);
      props.updateBrowser({ chrome_exe_path: selected });
    }
  };

  const selectImagePath = async (onSelected: (path: string) => void) => {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "图片文件",
          extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"],
        },
      ],
    });

    if (typeof selected === "string") {
      onSelected(selected);
    }
  };

  const resourceTypeOptions = () => [
    { value: "Text", label: "文本" },
    { value: "Image", label: "图片" },
    { value: "LLM", label: "LLM" },
  ];

  // 正则回复模板仅支持静态文本/图片；LLM 路径由上方的「回复策略」控制，避免误选无效项
  const replyTemplateResourceTypeOptions = () => [
    { value: "Text", label: "文本" },
    { value: "Image", label: "图片" },
  ];

  const greetTemplateHasLlm = props.config.greet_config.default_template.some(
    (resource) => resource.resource_type === "LLM",
  );
  const greetTemplateHasActiveImage =
    props.config.greet_config.default_template.some(
      (resource) =>
        resource.resource_type === "Image" && resource.enabled !== false,
    );

  const addGreetLlmResource = () => props.addGreetDefaultResource("LLM");

  const renderResourceContent = (
    resource: ReplyResource,
    textPlaceholder: string,
    imagePathUpdater: (path: string) => void,
    contentUpdater: (content: string) => void,
  ) => {
    if (resource.resource_type === "Text") {
      return (
        <Input.TextArea
          rows={3}
          value={resource.content}
          placeholder={textPlaceholder}
          onChange={(e) => contentUpdater(e.target.value)}
          className="font-mono text-sm"
        />
      );
    }

    if (resource.resource_type === "LLM") {
      return (
        <div className="flex h-8 items-center text-slate-400">
          <span className="text-xs tracking-wider">
            该内容将在运行时由 LLM 动态生成
          </span>
        </div>
      );
    }

    return (
      <Space.Compact className="w-full">
        <Input
          readOnly
          value={resource.content}
          placeholder="选择图片路径"
          className="font-mono text-sm"
        />
        <Button onClick={() => selectImagePath(imagePathUpdater)}>
          选择图片
        </Button>
      </Space.Compact>
    );
  };

  const selectResumePdf = async () => {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "PDF 简历", extensions: ["pdf"] }],
    });

    if (typeof selected !== "string") {
      return;
    }

    const loading = noticeModal.info({
      title: "正在解析 PDF 简历...",
      content: (
        <Space align="start" size={8}>
          <LoadingOutlined className="mt-1 text-sky-500" spin />
          <span>请稍候，解析完成后会自动写回配置。</span>
        </Space>
      ),
      centered: true,
      okButtonProps: { style: { display: "none" } },
      closable: false,
      maskClosable: false,
      keyboard: false,
    });
    try {
      const result = await invoke<CommandResult<string>>("parse_resume_pdf", {
        path: selected,
      });
      if (!result.success || result.data === null) {
        throw new Error(commandErrorMessage(result.error, "简历解析失败"));
      }

      form.setFieldsValue({
        resume_config: {
          resume_path: selected,
          resume_content: result.data,
        },
      });
      props.updateResume({
        resume_path: selected,
        resume_content: result.data,
      });
      showNotice("success", "简历解析完成", "简历已解析完成，内容已写回配置。");
    } catch (error: unknown) {
      showNotice(
        "error",
        "简历解析失败",
        error instanceof Error ? error.message : "简历解析失败",
      );
    } finally {
      loading.destroy();
    }
  };

  // 当外部配置改变时（如首次加载），更新表单
  useEffect(() => {
    // 转换城市和行业代码为 Cascader 要求的路径格式
    const config = { ...props.config };
    const jobFilter = { ...config.job_filter_config };

    // 全国是单层虚拟节点；普通城市仍是 Cascader 路径。
    if (jobFilter.city === nationwideCityCode) {
      (jobFilter as any).city = [nationwideCityCode];
    } else if (jobFilter.city) {
      for (const prov of cityTreeOptions) {
        const city = prov.children?.find((c) => c.value === jobFilter.city);
        if (city) {
          (jobFilter as any).city = [prov.value, city.value];
          break;
        }
      }
    }

    // 还原行业路径 [[parentCode, childCode], ...]
    if (jobFilter.industry && jobFilter.industry.length > 0) {
      const industryPaths: number[][] = [];
      for (const code of jobFilter.industry) {
        let found = false;
        for (const cat of industryTreeOptions) {
          const sub = cat.children?.find((s) => s.value === code);
          if (sub) {
            industryPaths.push([cat.value, sub.value]);
            found = true;
            break;
          }
        }
        if (!found) industryPaths.push([code]); // 兜底
      }
      (jobFilter as any).industry = industryPaths;
    }

    form.setFieldsValue({
      ...config,
      job_filter_config: jobFilter,
    });
  }, [props.config, form]);

  const renderContent = () => {
    switch (activeGroup) {
      case "runtime":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <SafetyOutlined className="text-sky-500" />
                运行与防封
              </Title>
              <Text className="text-slate-500 text-xs">
                全局运行节奏、账号防封护栏、自动回复轮询与实时通知（跨所有求职方案生效）
              </Text>
            </div>
            <Divider className="!my-0 opacity-10" />
            <PeriodicDeliverySection
              config={getPeriodicDeliveryConfig(props.config)}
              onChange={props.updatePeriodicDelivery}
              resetTo={DEFAULT_PERIODIC_DELIVERY_CONFIG}
              resetLabel="恢复出厂默认"
            />
            <HumanizeSection
              config={getHumanizeConfig(props.config)}
              onChange={props.updateHumanize}
            />
            <ReplyPollingSection
              config={getReplyPollingConfig(props.config)}
              onChange={props.updatePolling}
            />
            <NotificationSection
              config={getNotificationConfig(props.config)}
              onChange={props.updateNotification}
            />
          </Space>
        );
      case "llm":
        return (
          <LlmConfigPanel
            config={props.config.llm_config}
            enabled={llmEnabled}
            onChange={props.updateLlm}
            onEnabledChange={(enabled) =>
              props.updateLlmEnabled(enabled ? undefined : false)
            }
            onPersist={props.persistLlm}
            onPersistAll={props.persistConfig}
            fallbacks={props.config.llm_fallbacks}
            onFallbacksChange={props.updateLlmFallbacks}
            retryConfig={props.config.llm_retry_config}
            onRetryConfigChange={props.updateLlmRetryConfig}
            dirty={props.dirty}
          />
        );
      case "data":
        return <DataManagementPanel onConfigImported={props.onConfigImported} />;
      case "job":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <Title
                  level={4}
                  className="text-slate-900! m-0! flex items-center gap-2"
                >
                  <ProductOutlined className="text-sky-500" />
                  岗位筛选
                </Title>
                <Text className="text-slate-500 text-xs">
                  配置检索条件、反噪黑名单、真实薪资护栏与平台专属过滤
                </Text>
              </div>
              <Segmented<"basic" | "guard" | "platform">
                value={jobFilterTab}
                onChange={setJobFilterTab}
                options={[
                  { value: "basic", label: "基础与关键词" },
                  { value: "guard", label: "反噪与薪资" },
                  { value: "platform", label: "平台专属与 AI" },
                ]}
              />
            </div>
            <Divider className="!my-0 opacity-10" />
            <div className={jobFilterTab === "basic" ? undefined : "hidden"}>
            <Row gutter={[24, 16]}>
              <Col xs={24} md={12}>
                <Form.Item
                  label="岗位关键词"
                  name={["job_filter_config", "query"]}
                >
                  <Input
                    placeholder="输入搜索关键词"
                    onChange={(e) =>
                      props.updateJobFilter({ query: e.target.value || null })
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24}>
                <Form.Item label="AI 提取多个筛选关键词">
                  <Space.Compact block>
                    <Input
                      value={keywordRequirement}
                      placeholder="例如：岗位要 Java、Golang、数据分析，公司排除外包和保险公司"
                      onChange={(event) =>
                        setKeywordRequirement(event.target.value)
                      }
                    />
                    <Button
                      type="primary"
                      icon={<RobotOutlined />}
                      loading={parsingKeywords}
                      disabled={!llmActive}
                      onClick={() => void parseJobKeywordFilters()}
                    >
                      提取
                    </Button>
                  </Space.Compact>
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="目标城市"
                  name={["job_filter_config", "city"]}
                >
                  <Cascader
                    options={cityTreeOptions}
                    placeholder="选择目标城市"
                    showSearch
                    expandTrigger="hover"
                    onChange={(v) =>
                      props.updateJobFilter({
                        city: (v?.[v.length - 1] as number) || null,
                      })
                    }
                    displayRender={(labels) => labels[labels.length - 1]}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="求职类型"
                  name={["job_filter_config", "job_type"]}
                >
                  <Select
                    options={jobTypeOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) => props.updateJobFilter({ job_type: v })}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="薪资范围"
                  name={["job_filter_config", "salary"]}
                >
                  <Select
                    options={salaryOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) => props.updateJobFilter({ salary: v })}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="工作经验要求 (多选)"
                  name={["job_filter_config", "experience"]}
                >
                  <Select
                    mode="multiple"
                    placeholder="选择经验要求"
                    options={experienceOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) => props.updateJobFilter({ experience: v })}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="最低学历 (多选)"
                  name={["job_filter_config", "dgree"]}
                >
                  <Select
                    mode="multiple"
                    placeholder="选择学历"
                    options={degreeOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) => props.updateJobFilter({ dgree: v })}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="融资阶段 (多选)"
                  name={["job_filter_config", "stage"]}
                >
                  <Select
                    mode="multiple"
                    placeholder="选择融资阶段"
                    options={stageOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) =>
                      props.updateJobFilter({
                        stage: normalizeMultiSelectCodes(v),
                      })
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="企业规模 (多选)"
                  name={["job_filter_config", "scale"]}
                >
                  <Select
                    mode="multiple"
                    placeholder="选择企业规模"
                    options={scaleOptions.map((o) => ({
                      value: o.code,
                      label: o.name,
                    }))}
                    onChange={(v) =>
                      props.updateJobFilter({
                        scale: normalizeMultiSelectCodes(v),
                      })
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24}>
                <Form.Item
                  label="行业选择 (可多选)"
                  name={["job_filter_config", "industry"]}
                >
                  <Cascader
                    multiple
                    options={industryTreeOptions}
                    placeholder="搜索并选择行业"
                    showSearch
                    expandTrigger="hover"
                    maxTagCount="responsive"
                    onChange={(v: (number | undefined)[][]) => {
                      // v is an array of arrays [[parent, child], [parent, child]]
                      const codes = v.map(
                        (path) => path[path.length - 1] as number,
                      );
                      props.updateJobFilter({ industry: codes });
                    }}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="包含关键词"
                  name={["job_filter_config", "keywords"]}
                >
                  <Select
                    mode="tags"
                    placeholder="输入并回车添加包含词"
                    onChange={(v) => props.updateJobFilter({ keywords: v })}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="排除关键词"
                  name={["job_filter_config", "exclude_keywords"]}
                >
                  <Select
                    mode="tags"
                    placeholder="输入并回车添加排除词"
                    onChange={(v) =>
                      props.updateJobFilter({ exclude_keywords: v })
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="包含公司关键词"
                  name={["job_filter_config", "company_keywords"]}
                >
                  <Select
                    mode="tags"
                    placeholder="输入并回车添加公司包含词"
                    onChange={(v) =>
                      props.updateJobFilter({ company_keywords: v })
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  label="排除公司关键词"
                  name={["job_filter_config", "company_exclude_keywords"]}
                >
                  <Select
                    mode="tags"
                    placeholder="输入并回车添加公司排除词"
                    onChange={(v) =>
                      props.updateJobFilter({ company_exclude_keywords: v })
                    }
                  />
                </Form.Item>
              </Col>
            </Row>
            </div>

            <div className={jobFilterTab === "guard" ? "space-y-4" : "hidden"}>
              <div className="rounded-xl border border-slate-200/80 bg-slate-50/70 px-4 py-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex items-center gap-1.5 text-sm font-semibold text-slate-700">
                    <StopOutlined className="text-rose-500" />
                    <span>一键导入反噪黑名单预设</span>
                    <Text type="secondary" className="font-normal text-xs">
                      （增量去重追加到「排除关键词 / 排除公司」，不会清空已填词）
                    </Text>
                  </div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {BLACKLIST_PRESETS.map((preset) => (
                    <Button
                      key={preset.id}
                      size="small"
                      title={preset.description}
                      onClick={() => {
                        const existing =
                          props.config.job_filter_config[preset.target] ?? [];
                        const merged = mergeUniqueKeywords(
                          existing,
                          preset.keywords,
                        );
                        const added = merged.length - existing.length;
                        props.updateJobFilter({ [preset.target]: merged });
                        showNotice(
                          "success",
                          `已应用「${preset.label}」`,
                          added > 0
                            ? `新增 ${added} 个过滤词，切回「基础与关键词」可随时微调或删除。`
                            : "该预设词已全部存在于你的黑名单中。",
                        );
                      }}
                    >
                      + {preset.label}
                    </Button>
                  ))}
                </div>
              </div>

            <Card size="small" className="border-emerald-500/25! bg-emerald-500/5!">
              <div className="mb-3 flex items-center gap-2">
                <DollarOutlined className="text-emerald-600" />
                <div>
                  <Text strong>真实薪资过滤与同公司防骚扰（零点击前置生效）</Text>
                  <div className="text-xs text-slate-500">
                    在点开岗位详情前直接过滤低薪、虚标宽跨度、面议及同公司重复建联，既省时间又降风控。
                  </div>
                </div>
              </div>
              <Row gutter={[16, 12]}>
                <Col xs={24} md={6}>
                  <Form.Item
                    label="最低月薪要求 (K)"
                    className="mb-0!"
                    extra="按薪资上限比对，0 表示不限"
                  >
                    <NumberField
                      min={0}
                      max={300}
                      precision={0}
                      fallback={0}
                      value={props.config.job_filter_config.min_salary_k ?? 0}
                      style={{ width: "100%" }}
                      onChange={(value) =>
                        props.updateJobFilter({
                          min_salary_k: value > 0 ? value : null,
                        })
                      }
                    />
                  </Form.Item>
                </Col>
                <Col xs={24} md={6}>
                  <Form.Item
                    label="薪资跨度虚标上限"
                    className="mb-0!"
                    extra="拦截如 10-30K (3x) 虚标岗"
                  >
                    <Select
                      value={props.config.job_filter_config.max_salary_span_ratio ?? 0}
                      options={[
                        { value: 0, label: "不限制" },
                        { value: 1.8, label: "严格（≤ 1.8 倍）" },
                        { value: 2.0, label: "标准（≤ 2.0 倍）" },
                        { value: 2.5, label: "宽松（≤ 2.5 倍）" },
                        { value: 3.0, label: "仅拦极端（≤ 3.0 倍）" },
                      ]}
                      onChange={(value) =>
                        props.updateJobFilter({
                          max_salary_span_ratio: value > 0 ? value : null,
                        })
                      }
                    />
                  </Form.Item>
                </Col>
                <Col xs={24} md={6}>
                  <Form.Item
                    label="同公司最多建联数"
                    className="mb-0!"
                    extra="0 = 不限制；建议 2~3 个"
                  >
                    <NumberField
                      min={0}
                      max={20}
                      precision={0}
                      fallback={0}
                      value={props.config.job_filter_config.max_greets_per_company ?? 0}
                      style={{ width: "100%" }}
                      onChange={(value) =>
                        props.updateJobFilter({
                          max_greets_per_company: value,
                        })
                      }
                    />
                  </Form.Item>
                </Col>
                <Col xs={24} md={6}>
                  <Form.Item
                    label="排除“薪资面议”"
                    className="mb-0!"
                    extra="跳过未标明数字薪资的岗位"
                  >
                    <div className="pt-1">
                      <Switch
                        checked={Boolean(props.config.job_filter_config.exclude_negotiable)}
                        onChange={(checked) =>
                          props.updateJobFilter({ exclude_negotiable: checked })
                        }
                      />
                    </div>
                  </Form.Item>
                </Col>
              </Row>
            </Card>
            </div>

            <div className={jobFilterTab === "platform" ? "space-y-4" : "hidden"}>
            <Job51FilterSection value={props.config.platform_filter_config.job51}
              onChange={job51 => props.updatePlatformFilter({ job51 })} />

            <Card size="small" className="mt-4 border-sky-500/25! bg-sky-500/5!">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <Text strong>BOSS 活跃度过滤</Text>
                  <div className="mt-1 text-xs leading-5 text-slate-500">
                    跳过长时间不活跃的招聘者，优先处理更可能回复的岗位。
                  </div>
                </div>
                <Switch
                  checked={props.config.platform_filter_config.boss.active_filter_enabled}
                  onChange={(checked) =>
                    props.updatePlatformFilter({
                      boss: {
                        ...props.config.platform_filter_config.boss,
                        active_filter_enabled: checked,
                      },
                    })
                  }
                />
              </div>
              {props.config.platform_filter_config.boss.active_filter_enabled && (
                <Row gutter={[16, 12]} className="mt-4">
                  <Col xs={24}>
                    <Form.Item
                      label="最低活跃要求"
                      className="mb-0!"
                    >
                      <BossActiveThresholdScale
                        value={props.config.platform_filter_config.boss.active_threshold}
                        onChange={(active_threshold) =>
                          props.updatePlatformFilter({
                            boss: {
                              ...props.config.platform_filter_config.boss,
                              active_threshold,
                            },
                          })
                        }
                      />
                    </Form.Item>
                  </Col>
                </Row>
              )}
            </Card>

            <Card size="small" className="mt-4 border-amber-500/25! bg-amber-500/5!">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <Text strong>BOSS 猎头岗位过滤</Text>
                  <div className="mt-1 text-xs leading-5 text-slate-500">
                    跳过带猎头标识的岗位，减少对不适合应届生岗位的投递。
                  </div>
                </div>
                <Switch
                  checked={props.config.platform_filter_config.boss.exclude_headhunter_jobs}
                  onChange={(checked) =>
                    props.updatePlatformFilter({
                      boss: {
                        ...props.config.platform_filter_config.boss,
                        exclude_headhunter_jobs: checked,
                      },
                    })
                  }
                />
              </div>
            </Card>

            <Card
              size="small"
              className="mt-4 border-violet-500/25! bg-violet-500/5!"
            >
              <div className="flex items-start justify-between gap-4">
                <div>
                  <Text strong>AI 岗位意图复核</Text>
                  <div className="mt-1 text-xs leading-5 text-slate-500">
                    关键词和正则规则通过后，再根据岗位职责复核一次；复核失败或模型异常时会跳过岗位，避免误投。
                  </div>
                </div>
                <Switch
                  checked={
                    props.config.job_filter_config.enable_semantic_filter
                  }
                  onChange={(checked) =>
                    props.updateJobFilter({ enable_semantic_filter: checked })
                  }
                />
              </div>
              {props.config.job_filter_config.enable_semantic_filter && (
                <Form.Item
                  className="mb-0! mt-4"
                  label="目标岗位要求"
                  name={["job_filter_config", "semantic_filter_intent"]}
                  extra={
                    llmActive
                      ? "建议同时写清希望投递和明确排除的岗位方向。"
                      : llmConfigured
                        ? "大模型当前已停用，启动任务前会提示先启用。"
                        : "尚未配置大模型，启动任务前会提示配置。"
                  }
                >
                  <Input.TextArea
                    autoSize={{ minRows: 3, maxRows: 6 }}
                    placeholder="例如：只投 AI 应用开发、Agent 工程师；需要以编码和系统落地为主。不投产品经理、销售、运营、纯算法研究岗位。"
                    onChange={(event) =>
                      props.updateJobFilter({
                        semantic_filter_intent: event.target.value || null,
                      })
                    }
                  />
                </Form.Item>
              )}
            </Card>

            <Collapse
              ghost
              className="mt-4"
              items={[
                {
                  key: "rules",
                  label: (
                    <div className="group flex w-full cursor-pointer items-center gap-2 text-slate-500 transition-colors hover:text-sky-600">
                      <ControlOutlined className="shrink-0 text-xs" />
                      <span className="text-xs font-bold uppercase tracking-widest whitespace-nowrap shrink-0">
                        高级筛选：正则匹配规则
                      </span>
                      <Divider className="!m-0 flex-1 opacity-10 min-w-[40px]" />
                    </div>
                  ),
                  children: (
                    <div className="space-y-6 pt-4">
                      <Card
                        size="small"
                        className="border-sky-500/25! bg-sky-500/5!"
                      >
                        <div className="flex flex-col gap-3">
                          <div>
                            <Text strong className="text-slate-800">
                              <RobotOutlined className="mr-2 text-sky-600" />
                              AI 生成高级规则
                            </Text>
                            <Text className="mt-1 block text-xs text-slate-500">
                              用自然语言描述想保留或排除的岗位，生成结果会追加到下方，检查后再保存。
                            </Text>
                          </div>
                          <div className="mb-3">
                            <Input.TextArea
                              value={ruleRequirement}
                              onChange={(event) =>
                                setRuleRequirement(event.target.value)
                              }
                              placeholder="例如：只看 Java 或 Golang 后端岗位，排除外包、驻场和保险公司"
                              autoSize={{ minRows: 2, maxRows: 5 }}
                              maxLength={1000}
                              showCount
                            />
                          </div>
                          <div className="flex justify-end">
                            <Button
                              type="primary"
                              icon={<RobotOutlined />}
                              loading={generatingRules}
                              onClick={() => void generateJobFilterRules()}
                            >
                              {llmActive
                                ? "生成规则"
                                : llmConfigured
                                  ? "先启用大模型"
                                  : "先配置大模型"}
                            </Button>
                          </div>
                        </div>
                      </Card>

                      <div className="flex items-center justify-between">
                        <Text className="text-slate-500 text-[10px] font-bold uppercase tracking-tighter">
                          添加正则表达式来精准包含或拒绝特定岗位字段
                        </Text>
                        <Button
                          type="text"
                          size="small"
                          icon={<PlusOutlined />}
                          onClick={(e) => {
                            e.stopPropagation();
                            props.addRule();
                          }}
                          className="rounded-lg! text-sky-500! hover:bg-[var(--of-accent-soft)]! font-bold"
                        >
                          新增规则
                        </Button>
                      </div>

                      <div className="space-y-3">
                        {props.config.job_filter_config.regex_rules.map(
                          (rule, index) => (
                            <Card
                              key={index}
                              size="small"
                              className="bg-[var(--of-surface)]! hover:bg-[var(--of-surface-hover)]! transition-colors border-[var(--of-border)]! relative group/card"
                            >
                              <div className="flex flex-wrap gap-3 items-end pr-10">
                                <div className="flex-1 min-w-[120px]">
                                  <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                    规则名称
                                  </Text>
                                  <Input
                                    size="small"
                                    value={rule.name}
                                    onChange={(e) =>
                                      props.updateRule(index, {
                                        name: e.target.value,
                                      })
                                    }
                                    className="!bg-transparent"
                                  />
                                </div>
                                <div className="flex-[2] min-w-[180px]">
                                  <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                    正则表达式
                                  </Text>
                                  <Input
                                    size="small"
                                    value={rule.pattern}
                                    onChange={(e) =>
                                      props.updateRule(index, {
                                        pattern: e.target.value,
                                      })
                                    }
                                    className="font-mono !bg-transparent"
                                  />
                                </div>
                                <div className="flex-1 min-w-[100px]">
                                  <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                    匹配目标
                                  </Text>
                                  <Select
                                    size="small"
                                    value={rule.target}
                                    onChange={(v) =>
                                      props.updateRule(index, {
                                        target: v as MatchTarget,
                                      })
                                    }
                                    className="w-full"
                                    options={[
                                      { value: "Title", label: "标题" },
                                      { value: "Company", label: "公司" },
                                      { value: "Description", label: "描述" },
                                      { value: "All", label: "全部" },
                                    ]}
                                  />
                                </div>
                                <div className="flex-1 min-w-[80px]">
                                  <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                    逻辑
                                  </Text>
                                  <Select
                                    size="small"
                                    value={rule.mode}
                                    onChange={(v) =>
                                      props.updateRule(index, {
                                        mode: v as RuleMode,
                                      })
                                    }
                                    className="w-full"
                                    options={[
                                      { value: "ACCEPT", label: "接受" },
                                      { value: "REJECT", label: "拒绝" },
                                    ]}
                                  />
                                </div>
                              </div>
                              <Button
                                type="text"
                                danger
                                size="small"
                                icon={<DeleteOutlined />}
                                className="absolute top-2 right-2 opacity-0 group-hover/card:opacity-100 transition-opacity"
                                onClick={() => props.removeRule(index)}
                              />
                            </Card>
                          ),
                        )}
                        {props.config.job_filter_config.regex_rules.length ===
                          0 && (
                          <div className="py-12 rounded-2xl border border-dashed border-slate-300 bg-slate-50 flex items-center justify-center">
                            <Empty
                              image={Empty.PRESENTED_IMAGE_SIMPLE}
                              description={
                                <Text className="text-slate-400 font-bold uppercase tracking-widest text-[10px]">
                                  未配置高级规则
                                </Text>
                              }
                            />
                          </div>
                        )}
                      </div>
                    </div>
                  ),
                },
              ]}
            />
            </div>

            <Alert
              type="info"
              showIcon
              title="投递频率、每日安全上限与拟人化节奏已统一归口至「系统能力 · 运行与防封」"
              action={
                <Button size="small" type="link" onClick={() => setActiveGroup("runtime")}>
                  前往调整
                </Button>
              }
            />
          </Space>
        );
      case "greet":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <CommentOutlined className="text-sky-500" />
                打招呼配置
              </Title>
              <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
                配置主动沟通的 LLM 与兜底话术
              </Text>
            </div>
            
            <AiFeatureGate
              active={llmActive}
              configured={llmConfigured}
              onConfigure={props.onOpenLlmConfig}
            >
              <></>
            </AiFeatureGate>
            <div className="space-y-6">
              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
                <div className="flex items-center justify-between gap-6">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      LLM 主动沟通
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      需同时打开开关并填写下方提示词，才会调用大模型生成打招呼内容
                    </Text>
                  </div>
                  <Form.Item
                    name={["greet_config", "enable_llm"]}
                    valuePropName="checked"
                    className="!m-0"
                  >
                    <Switch
                      disabled={!llmActive && !props.config.greet_config.enable_llm}
                      onChange={(v) => props.updateGreet({ enable_llm: v })}
                    />
                  </Form.Item>
                </div>

                {props.config.greet_config.enable_llm && (
                  <div>
                    <Form.Item
                      label="主动沟通提示词"
                      name={["greet_config", "reply_prompt"]}
                    >
                      <Input.TextArea
                        rows={10}
                        placeholder="在此输入生成打招呼内容的 Prompt 模板..."
                        onChange={(e) =>
                          props.updateGreet({
                            reply_prompt: e.target.value || null,
                          })
                        }
                        className="font-mono text-sm"
                      />
                    </Form.Item>
                    <PromptVariableGuide
                      items={buildCommonPromptVariables(
                        props.config.resume_config.inject_llm_context,
                      )}
                    />
                  </div>
                )}
              </div>

              {props.config.greet_config.enable_llm && !greetTemplateHasLlm && (
                <Alert
                  type="warning"
                  showIcon
                  message="发送序列中尚未添加 LLM 内容"
                  description="大模型不会被隐式插入发送。添加 LLM 内容并启用后，才会在对应位置生成并发送。"
                  action={
                    <Button
                      size="small"
                      type="link"
                      onClick={addGreetLlmResource}
                      className="font-bold"
                    >
                      添加 LLM 内容
                    </Button>
                  }
                />
              )}

              {greetTemplateHasActiveImage && (
                <Alert
                  type="warning"
                  showIcon
                  message="首轮打招呼包含图片消息，可能触发平台风控"
                  description="BOSS 直聘等平台对陌生会话首条发图检测较严，建议仅发简短文字破冰，待 HR 回复或索要简历后再通过平台原生附件发简历。"
                />
              )}

              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-4">
                <div className="flex items-center justify-between">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      打招呼发送顺序
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      仅启用的内容会按从上到下依次发送
                    </Text>
                  </div>
                  <Button
                    type="text"
                    icon={<PlusOutlined />}
                    onClick={() => props.addGreetDefaultResource()}
                    className="text-sky-500! hover:bg-[var(--of-accent-soft)]! rounded-lg! font-bold"
                  >
                    添加内容
                  </Button>
                </div>

                <div className="space-y-3">
                  {props.config.greet_config.default_template.map(
                    (resource, index) => (
                      <div
                        key={index}
                        className="grid grid-cols-1 md:grid-cols-[40px_120px_minmax(0,1fr)_auto] gap-3 items-start rounded-2xl border border-[var(--of-border)] bg-[var(--of-surface)] p-3 shadow-sm transition-shadow hover:shadow-md"
                      >
                        <div className="flex h-8 items-center justify-center rounded-lg bg-[var(--of-accent-soft)] text-xs font-bold text-[var(--of-accent)]">
                          {index + 1}
                        </div>
                        <Select
                          value={resource.resource_type}
                          onChange={(value) =>
                            props.updateGreetDefaultResource(index, {
                              resource_type:
                                value as GreetResource["resource_type"],
                            })
                          }
                          options={resourceTypeOptions().map((option) => ({
                            ...option,
                            disabled:
                              option.value === "LLM" &&
                              greetTemplateHasLlm &&
                              resource.resource_type !== "LLM",
                          }))}
                        />
                        {renderResourceContent(
                          resource,
                          "输入默认打招呼文本",
                          (path) =>
                            props.updateGreetDefaultResource(index, {
                              content: path,
                            }),
                          (content) =>
                            props.updateGreetDefaultResource(index, {
                              content,
                            }),
                        )}
                        <div className="flex items-center justify-self-end rounded-xl border border-[var(--of-border)] bg-[var(--of-surface-subtle)] p-1 shadow-inner">
                          <div className="flex items-center gap-0.5 pr-1">
                            <Button
                              type="text"
                              title="上移"
                              aria-label={`上移第 ${index + 1} 条打招呼内容`}
                              icon={<ArrowUpOutlined />}
                              disabled={index === 0}
                              onClick={() =>
                                props.moveGreetDefaultResource(index, -1)
                              }
                              className="h-8! w-8! min-w-8! rounded-lg! p-0! text-[var(--of-text-secondary)]! hover:bg-[var(--of-surface)]! hover:shadow-sm!"
                            />
                            <Button
                              type="text"
                              title="下移"
                              aria-label={`下移第 ${index + 1} 条打招呼内容`}
                              icon={<ArrowDownOutlined />}
                              disabled={
                                index ===
                                props.config.greet_config.default_template
                                  .length -
                                  1
                              }
                              onClick={() =>
                                props.moveGreetDefaultResource(index, 1)
                              }
                              className="h-8! w-8! min-w-8! rounded-lg! p-0! text-[var(--of-text-secondary)]! hover:bg-[var(--of-surface)]! hover:shadow-sm!"
                            />
                          </div>
                          <div className="h-5 w-px bg-[var(--of-border)]" />
                          <Button
                            title={
                              resource.enabled === false
                                ? "启用发送"
                                : "停用发送"
                            }
                            aria-label={`${
                              resource.enabled === false ? "启用" : "停用"
                            }第 ${index + 1} 条打招呼内容`}
                            type="text"
                            className={
                              resource.enabled === false
                                ? "mx-1! h-8! w-8! min-w-8! rounded-lg! p-0! text-[var(--of-text-secondary)]! hover:bg-[var(--of-surface)]!"
                                : "mx-1! h-8! w-8! min-w-8! rounded-lg! bg-emerald-500/12! p-0! text-emerald-500! hover:bg-emerald-500/20!"
                            }
                            icon={
                              resource.enabled === false ? (
                                <EyeInvisibleOutlined />
                              ) : (
                                <EyeOutlined />
                              )
                            }
                            onClick={() =>
                              props.updateGreetDefaultResource(index, {
                                enabled: resource.enabled === false,
                              })
                            }
                          />
                          <div className="h-5 w-px bg-[var(--of-border)]" />
                          <Button
                            type="text"
                            title="删除"
                            aria-label={`删除第 ${index + 1} 条打招呼内容`}
                            danger
                            icon={<DeleteOutlined />}
                            onClick={() =>
                              props.removeGreetDefaultResource(index)
                            }
                            className="ml-1! h-8! w-8! min-w-8! rounded-lg! p-0! hover:bg-red-500/12!"
                          />
                        </div>
                      </div>
                    ),
                  )}

                  {props.config.greet_config.default_template.length === 0 && (
                    <div className="py-10 rounded-2xl border border-dashed border-slate-300 bg-slate-50 flex items-center justify-center">
                      <Empty
                        image={Empty.PRESENTED_IMAGE_SIMPLE}
                        description={
                          <Text className="text-slate-400 font-bold uppercase tracking-widest text-[10px]">
                            暂未配置默认打招呼内容
                          </Text>
                        }
                      />
                    </div>
                  )}
                </div>
              </div>
            </div>
          </Space>
        );
      case "analysis": {
        const analysis = getAnalysisConfig(props.config);
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <ThunderboltOutlined className="text-sky-500" />
                岗位分析
              </Title>
              <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
              配置 AI 自动评估岗位匹配度
              </Text>
            </div>
            
            <AiFeatureGate
              active={llmActive}
              configured={llmConfigured}
              onConfigure={props.onOpenLlmConfig}
            >
              <></>
            </AiFeatureGate>

            <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
              <div>
                <Text className="text-slate-900 font-bold block">分析时机</Text>
                <Text className="text-slate-500 text-xs">
                  一个岗位在一轮流程里只会被一个时机命中，分析在后台进行，不会拖慢求职任务
                </Text>
              </div>

              <RadioCardGroup
                ariaLabel="分析时机"
                value={analysis.trigger}
                options={ANALYSIS_TRIGGER_OPTIONS.map((option) => ({
                  ...option,
                  disabled: option.needsLlm && !llmActive,
                }))}
                onChange={(trigger) => props.updateAnalysis({ trigger })}
              />

              {!llmActive && (
                <Text className="text-slate-400 text-xs block">
                  自动分析需要先启用模型服务后才能选择。
                </Text>
              )}
            </div>

            {analysis.trigger !== "off" && (
              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-4">
                <div>
                  <Text className="text-slate-900 font-bold block">
                    额度护栏
                  </Text>
                  <Text className="text-slate-500 text-xs">
                    一次分析就是一次完整的大模型调用，这两项决定它最多能花多少
                  </Text>
                </div>

                <SettingGroup>
                  <SettingToggle
                    icon={<ForwardOutlined />}
                    title="跳过已分析过的岗位"
                    description="关掉后同一个岗位会被反复分析并覆盖旧报告；解析失败的报告不受此项影响，始终会重跑"
                    checked={analysis.skip_analyzed}
                    onChange={(value) =>
                      props.updateAnalysis({ skip_analyzed: value })
                    }
                  />
                  <SettingSlider
                    icon={<ProfileOutlined />}
                    title="单次任务分析上限"
                    description="每次求职任务最多自动分析多少个岗位，拖到 0 表示不限制"
                    min={0}
                    max={MAX_ANALYSIS_PER_TASK}
                    sliderMax={100}
                    step={5}
                    fallback={DEFAULT_MAX_ANALYSIS_PER_TASK}
                    value={analysis.max_per_task}
                    unit="个岗位"
                    valueLabel={(value) => (value === 0 ? "不限制" : null)}
                    onChange={(value) =>
                      props.updateAnalysis({ max_per_task: value })
                    }
                  />
                </SettingGroup>
              </div>
            )}

            <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-4">
              <div>
                <Text className="text-slate-900 font-bold block">统计口径</Text>
                <Text className="text-slate-500 text-xs">
                  分析得分达到分数线的岗位，计入求职数据概览的「高匹配岗位」
                </Text>
              </div>

              <SettingGroup>
                <SettingSlider
                  icon={<RiseOutlined />}
                  title="高匹配分数线"
                  description="定得越高，概览里的高匹配岗位越少也越准"
                  min={MIN_HIGH_MATCH_SCORE}
                  max={100}
                  fallback={DEFAULT_HIGH_MATCH_SCORE}
                  value={analysis.high_match_score}
                  unit="分"
                  onChange={(value) =>
                    props.updateAnalysis({ high_match_score: value })
                  }
                />
              </SettingGroup>
              <Alert
                type="info"
                showIcon
                message="分析报告在岗位管理页打开岗位即可查看，也可以在那里勾选多个岗位批量分析存量数据。"
              />
            </div>
          </Space>
        );
      }
      case "reply": {
        const replayStrategy = replyStrategyOf(props.config.replay_config);
        const usesLlmReply =
          replayStrategy === "llm" || replayStrategy === "template_first";
        const usesTemplateReply =
          replayStrategy === "template" || replayStrategy === "template_first";
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <CommentOutlined className="text-sky-500" />
                自动回复
              </Title>
              <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
                配置 HR 对话中的自动回复策略
              </Text>
            </div>
            
            <AiFeatureGate
              active={llmActive}
              configured={llmConfigured}
              onConfigure={props.onOpenLlmConfig}
            >
              <></>
            </AiFeatureGate>
            <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
              <div>
                <Text className="text-slate-900 font-bold block">回复策略</Text>
                <Text className="text-slate-500 text-xs">
                  未读会话按这里选定的方式处理，每条消息只会走其中一条路径
                </Text>
              </div>

              <RadioCardGroup
                ariaLabel="回复策略"
                value={replayStrategy}
                options={REPLY_STRATEGY_OPTIONS.map((option) => ({
                  ...option,
                  disabled: option.needsLlm && !llmActive,
                }))}
                onChange={(strategy) =>
                  props.updateReplay(REPLY_STRATEGY_FLAGS[strategy])
                }
              />

              {!llmActive && (
                <Text className="text-slate-400 text-xs block">
                  含 AI 的策略需要先启用模型服务后才能选择。
                </Text>
              )}

              {replayStrategy !== "off" && (
                <div className="flex items-center justify-between gap-6 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      演练模式
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      照常判断并生成回复，但不实际发送，只写日志；建议首次使用先开启，跑一轮确认生成质量再关掉
                    </Text>
                  </div>
                  <Form.Item
                    name={["replay_config", "dry_run"]}
                    valuePropName="checked"
                    className="!m-0"
                  >
                    <Switch
                      aria-label="演练模式"
                      onChange={(v) => props.updateReplay({ dry_run: v })}
                    />
                  </Form.Item>
                </div>
              )}

              {usesLlmReply && (
                <div className="space-y-5">
                  <Form.Item
                    label="自动回复提示词"
                    name={["replay_config", "reply_prompt"]}
                  >
                    <Input.TextArea
                      rows={10}
                      placeholder="在此输入大模型回复的 Prompt 模板..."
                      onChange={(e) =>
                        props.updateReplay({
                          reply_prompt: e.target.value || null,
                        })
                      }
                      className="font-mono text-sm"
                    />
                  </Form.Item>
                  <PromptVariableGuide
                    items={buildReplyPromptVariables(
                      props.config.resume_config.inject_llm_context,
                    )}
                  />

                  <Form.Item
                    label="背景上下文"
                    name={["replay_config", "background_context"]}
                    extra="补充简历里没有体现、但希望 LLM 回复时参考的信息。"
                  >
                    <Input.TextArea
                      rows={5}
                      placeholder="例如：更偏好远程协作、可接受短期出差、近期重点关注 AI 自动化方向..."
                      onChange={(e) =>
                        props.updateReplay({
                          background_context: e.target.value || null,
                        })
                      }
                      className="text-sm"
                    />
                  </Form.Item>

                  <div className="rounded-2xl border border-slate-200/80 bg-slate-50/80 p-5 space-y-4">
                    <div>
                      <Text className="text-slate-900 font-bold block">
                        自动回复边界
                      </Text>
                      <Text className="text-slate-500 text-xs">
                        限制模型的自主程度，超出边界的会话会转回人工
                      </Text>
                    </div>

                    <SettingGroup>
                      <SettingToggle
                        icon={<SolutionOutlined />}
                        title="允许自主投递简历"
                        description="关闭后模型仍会判断投递时机，但只回消息，投递交回人工"
                        checked={
                          props.config.replay_config.enable_auto_send_resume
                        }
                        onChange={(v) =>
                          props.updateReplay({ enable_auto_send_resume: v })
                        }
                      />
                      <SettingSlider
                        icon={<CommentOutlined />}
                        title="单会话回复上限"
                        description="同一会话在下方时段内最多自动回几条，用完交回给你"
                        min={1}
                        max={30}
                        fallback={DEFAULT_MAX_AUTO_REPLIES}
                        value={props.config.replay_config.max_auto_replies}
                        unit="条"
                        onChange={(value) =>
                          props.updateReplay({ max_auto_replies: value })
                        }
                      />
                      <SettingSlider
                        icon={<ClockCircleOutlined />}
                        title="额度计算时段"
                        description="滚动计算，时间过去之后额度自动恢复，不必手工重置"
                        min={1}
                        max={48}
                        fallback={DEFAULT_AUTO_REPLY_WINDOW_HOURS}
                        value={
                          props.config.replay_config.auto_reply_window_hours
                        }
                        unit="小时"
                        onChange={(value) =>
                          props.updateReplay({ auto_reply_window_hours: value })
                        }
                      />
                      <SettingSlider
                        icon={<FontSizeOutlined />}
                        title="单条回复字数上限"
                        description="超长的求职消息本身就不像真人写的，超出后自动截断"
                        min={50}
                        max={500}
                        step={10}
                        fallback={DEFAULT_MAX_REPLY_CHARS}
                        value={props.config.replay_config.max_reply_chars}
                        unit="字"
                        onChange={(value) =>
                          props.updateReplay({ max_reply_chars: value })
                        }
                      />
                    </SettingGroup>
                  </div>
                </div>
              )}

              <Alert
                type="info"
                showIcon
                message="自动回复的轮询间隔、活跃时段与实时推送提醒已归口至「系统能力 · 运行与防封」"
                action={
                  <Button size="small" type="link" onClick={() => setActiveGroup("runtime")}>
                    前往调整
                  </Button>
                }
              />
            </div>

            {usesTemplateReply && (
              <div className="space-y-6">
                <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      正则回复规则
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      {usesLlmReply
                        ? "按顺序匹配，命中的第一条规则直接发送固定话术；没有命中的消息交给 AI 回复"
                        : "按顺序匹配，命中的第一条规则直接发送固定话术；没有命中的消息留给人工处理"}
                    </Text>
                  </div>

                  <div className="space-y-4 pt-2 border-t border-slate-200/80">
                    <div className="flex items-center justify-between">
                      <div>
                        <Text className="text-slate-900 font-bold block">
                          正则匹配回复模板
                        </Text>
                        <Text className="text-slate-500 text-xs">
                          命中规则后按顺序发送文本或图片资源
                        </Text>
                      </div>
                      <Button
                        type="text"
                        icon={<PlusOutlined />}
                        onClick={props.addReplyTemplate}
                        className="text-sky-500! hover:bg-[var(--of-accent-soft)]! rounded-lg! font-bold"
                      >
                        新增模板
                      </Button>
                    </div>

                    {props.config.replay_config.templates.map(
                      (template, index) => (
                        <Card
                          key={index}
                          size="small"
                          className="bg-[var(--of-surface)]! border-[var(--of-border)]! relative overflow-hidden"
                        >
                          <div className="absolute inset-y-0 left-0 w-1 bg-linear-to-b from-cyan-500 to-emerald-500" />
                          <div className="space-y-4 pl-3">
                            <div className="flex items-center justify-between gap-3">
                              <Text className="text-xs font-black uppercase tracking-[0.18em] text-sky-600">
                                Template #{index + 1}
                              </Text>
                              <Button
                                type="text"
                                danger
                                size="small"
                                icon={<DeleteOutlined />}
                                onClick={() => props.removeReplyTemplate(index)}
                              />
                            </div>

                            <div className="grid grid-cols-1 xl:grid-cols-[1fr_1.6fr_160px] gap-3 items-end">
                              <div>
                                <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                  规则名称
                                </Text>
                                <Input
                                  size="small"
                                  value={template.regex_rule.name}
                                  onChange={(e) =>
                                    props.updateReplyTemplate(index, {
                                      regex_rule: {
                                        ...template.regex_rule,
                                        name: e.target.value,
                                      },
                                    })
                                  }
                                />
                              </div>
                              <div>
                                <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                  正则表达式
                                </Text>
                                <Input
                                  size="small"
                                  value={template.regex_rule.pattern}
                                  placeholder="例如: 简历|面试|岗位"
                                  onChange={(e) =>
                                    props.updateReplyTemplate(index, {
                                      regex_rule: {
                                        ...template.regex_rule,
                                        pattern: e.target.value,
                                      },
                                    })
                                  }
                                  className="font-mono"
                                />
                              </div>
                              <div>
                                <Text className="text-[10px] text-slate-500 font-bold block mb-1">
                                  匹配最近聊天条数
                                </Text>
                                <NumberField
                                  size="small"
                                  min={1}
                                  precision={0}
                                  fallback={DEFAULT_REGEX_RULE_LIMIT}
                                  value={template.regex_rule.limit}
                                  onChange={(value) =>
                                    props.updateReplyTemplate(index, {
                                      regex_rule: {
                                        ...template.regex_rule,
                                        limit: value,
                                      },
                                    })
                                  }
                                  className="!w-full"
                                />
                              </div>
                            </div>

                            <div className="space-y-3">
                              <div className="flex items-center justify-between">
                                <Text className="text-[10px] text-slate-500 font-bold uppercase tracking-widest">
                                  回复内容
                                </Text>
                                <Button
                                  type="text"
                                  size="small"
                                  icon={<PlusOutlined />}
                                  onClick={() => props.addReplyResource(index)}
                                  className="rounded-lg! text-sky-500! hover:bg-[var(--of-accent-soft)]! font-bold"
                                >
                                  添加内容
                                </Button>
                              </div>
                              {template.content.map(
                                (resource, resourceIndex) => (
                                  <div
                                    key={resourceIndex}
                                    className="grid grid-cols-1 md:grid-cols-[120px_1fr_40px] gap-3 items-start"
                                  >
                                    <Select
                                      value={
                                        resource.resource_type === "LLM"
                                          ? "Text"
                                          : resource.resource_type
                                      }
                                      onChange={(value) =>
                                        props.updateReplyResource(
                                          index,
                                          resourceIndex,
                                          {
                                            resource_type:
                                              value as ReplyResource["resource_type"],
                                          },
                                        )
                                      }
                                      options={replyTemplateResourceTypeOptions()}
                                    />
                                    {renderResourceContent(
                                      resource,
                                      "输入自动回复文本",
                                      (path) =>
                                        props.updateReplyResource(
                                          index,
                                          resourceIndex,
                                          {
                                            content: path,
                                          },
                                        ),
                                      (content) =>
                                        props.updateReplyResource(
                                          index,
                                          resourceIndex,
                                          { content },
                                        ),
                                    )}
                                    <Button
                                      type="text"
                                      danger
                                      icon={<DeleteOutlined />}
                                      onClick={() =>
                                        props.removeReplyResource(
                                          index,
                                          resourceIndex,
                                        )
                                      }
                                      disabled={template.content.length <= 1}
                                    />
                                  </div>
                                ),
                              )}
                            </div>
                          </div>
                        </Card>
                      ),
                    )}

                    {props.config.replay_config.templates.length === 0 && (
                      <div className="py-10 rounded-2xl border border-dashed border-slate-300 bg-slate-50 flex items-center justify-center">
                        <Empty
                          image={Empty.PRESENTED_IMAGE_SIMPLE}
                          description={
                            <Text className="text-slate-400 font-bold uppercase tracking-widest text-[10px]">
                              暂未配置回复模板
                            </Text>
                          }
                        />
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}
          </Space>
        );
      }
      case "browser":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <GlobalOutlined className="text-sky-500" />
                浏览器环境
              </Title>
              <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
                配置自动化运行时所需的本地路径
              </Text>
            </div>
            

            {browserEnvStatus && (
              <>
                {browserEnvStatus.browser_found ? (
                  <Alert
                    type="success"
                    showIcon
                    icon={<CheckCircleOutlined />}
                    message="浏览器已就绪"
                    description={
                      <span>
                        已检测到{" "}
                        <strong>{browserEnvStatus.browser_name}</strong>，路径：
                        <code className="text-xs bg-[var(--of-surface-subtle)] px-1.5 py-0.5 rounded">
                          {browserEnvStatus.browser_path}
                        </code>
                      </span>
                    }
                  />
                ) : (
                  <Alert
                    type="warning"
                    showIcon
                    icon={<WarningOutlined />}
                    message="未检测到浏览器"
                    description={
                      <div className="space-y-2">
                        <p className="m-0">
                          系统中未找到 Google Chrome 或 Microsoft
                          Edge，请手动配置浏览器可执行文件路径。
                        </p>
                        <div className="rounded-lg bg-amber-500/10 p-3 border border-amber-500/30">
                          <Text className="text-xs font-bold block mb-1.5 text-[var(--of-text)]">
                            <InfoCircleOutlined className="mr-1 text-amber-500" />
                            如何获取浏览器路径：
                          </Text>
                          <Text className="text-xs block text-[var(--of-text-secondary)]">
                            在浏览器地址栏输入{" "}
                            <code className="bg-amber-500/15 px-1 rounded">
                              chrome://version
                            </code>{" "}
                            或{" "}
                            <code className="bg-amber-500/15 px-1 rounded">
                              edge://version
                            </code>
                            ，找到「个人资料路径」或「可执行文件路径」，复制后粘贴到下方「浏览器可执行文件路径」中。
                          </Text>
                        </div>
                      </div>
                    }
                  />
                )}

                {!browserEnvStatus.user_data_dir_ok && (
                  <Alert
                    type="error"
                    showIcon
                    message="用户数据目录未配置"
                    description="请选择或输入用户数据目录路径，保存后将自动创建。"
                  />
                )}
              </>
            )}

            <Form.Item
              label="User Data Directory"
              name={["browser_config", "user_data_dir"]}
            >
              <Input
                placeholder="选择或输入本地用户数据目录路径"
                onChange={(e) =>
                  props.updateBrowser({ user_data_dir: e.target.value })
                }
                addonAfter={
                  <Button type="text" size="small" onClick={selectUserDataDir}>
                    选择目录
                  </Button>
                }
              />
            </Form.Item>
            <Form.Item
              label="浏览器可执行文件路径"
              name={["browser_config", "chrome_exe_path"]}
            >
              <Input
                placeholder="选择浏览器可执行文件路径（留空使用自动检测）"
                onChange={(e) =>
                  props.updateBrowser({
                    chrome_exe_path: e.target.value || null,
                  })
                }
                addonAfter={
                  <Button
                    type="text"
                    size="small"
                    onClick={selectChromeExePath}
                  >
                    选择文件
                  </Button>
                }
              />
            </Form.Item>
            <Form.Item
              label="最大并行任务数"
              name={["browser_config", "max_parallel_tasks"]}
              extra="默认为 3，BOSS、猎聘、前程无忧可各运行一个任务；同一平台的后续任务排队。"
            >
              <NumberField
                min={1}
                max={3}
                precision={0}
                fallback={DEFAULT_MAX_PARALLEL_TASKS}
                style={{ width: "100%" }}
                onChange={(value) =>
                  props.updateBrowser({ max_parallel_tasks: value })
                }
              />
            </Form.Item>
            <Alert
              type="info"
              showIcon
              message={`最多同时运行 ${props.config.browser_config.max_parallel_tasks} 个平台任务`}
              description="任务共享一个受管 Chrome，但使用独立连接和标签页；实际占用取决于页面、图片和聊天记录数量。"
            />
          </Space>
        );
      case "resume":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <Title
                  level={4}
                  className="text-slate-900! m-0! flex items-center gap-2"
                >
                  <FilePdfOutlined className="text-sky-500" />
                  简历配置
                </Title>
                <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
                  选择 PDF 简历并解析为可用于后续自动化的文本内容
                </Text>
              </div>
            </div>
            <AiFeatureGate
              active={llmActive}
              configured={llmConfigured}
              onConfigure={props.onOpenLlmConfig}
            >
              <></>
            </AiFeatureGate>
            
            <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6">
              <div className="flex items-center justify-between gap-6">
                <div>
                  <Text className="text-slate-900 font-bold block">
                    注入 LLM 上下文
                  </Text>
                  <Text className="text-slate-500 text-xs">
                    启用后将简历文本提供给打招呼和自动回复生成流程
                  </Text>
                </div>
                <Form.Item
                  name={["resume_config", "inject_llm_context"]}
                  valuePropName="checked"
                  className="!m-0"
                >
                  <Switch
                    aria-label="注入 LLM 上下文"
                    onChange={(value) =>
                      props.updateResume({ inject_llm_context: value })
                    }
                  />
                </Form.Item>
              </div>
            </div>
            <Form.Item label="简历附件" name={["resume_config", "resume_path"]}>
              <Input
                readOnly
                placeholder="请选择 PDF 简历文件"
                addonAfter={
                  <Button type="link" size="small" onClick={selectResumePdf}>
                    选择 PDF
                  </Button>
                }
              />
            </Form.Item>
            <Form.Item
              label="简历内容"
              name={["resume_config", "resume_content"]}
            >
              <Input.TextArea
                rows={16}
                placeholder="选择 PDF 后会自动解析文本，也可以在这里手动调整"
                onChange={(event) =>
                  props.updateResume({
                    resume_content: event.target.value || null,
                  })
                }
              />
            </Form.Item>
            <Form.Item
              label="猎聘附件简历名称"
              name={["resume_config", "liepin_attachment_resume_name"]}
              extra="优先选择完整名称匹配的附件。名称写错或留空时，根据岗位与附件预览正文匹配；信息不足时转人工。"
            >
              <Input
                allowClear
                placeholder="例如：张三_测试开发工程师简历"
                onChange={(event) =>
                  props.updateResume({
                    liepin_attachment_resume_name: event.target.value || null,
                  })
                }
              />
            </Form.Item>
            <Form.Item
              label="BOSS 附件简历名称"
              name={["resume_config", "boss_attachment_resume_name"]}
              extra="优先选择完整名称匹配的附件（含扩展名）。名称写错或留空时，根据岗位与附件预览正文匹配；信息不足时转人工。"
            >
              <Input
                allowClear
                placeholder="例如：张三简历_后端工程师_2026.pdf"
                onChange={(event) =>
                  props.updateResume({
                    boss_attachment_resume_name: event.target.value || null,
                  })
                }
              />
            </Form.Item>
          </Space>
        );
      case "about":
        return <AboutPanel />;
    }
  };

  const saveStatusBadge = (
    <div className="rounded-xl border border-slate-200/80 bg-white px-3 py-1.5 text-xs text-slate-500">
      {props.status === "loading" ? (
        <>
          <LoadingOutlined className="mr-1.5 text-sky-500" />
          正在自动保存…
        </>
      ) : props.status === "error" ? (
        <>
          <WarningOutlined className="mr-1.5 text-red-500" />
          {props.message || "自动保存失败，修改后将重试"}
        </>
      ) : props.dirty ? (
        <>
          <LoadingOutlined className="mr-1.5 text-sky-500" />
          等待自动保存…
        </>
      ) : (
        <>
          <CheckCircleOutlined className="mr-1.5 text-emerald-500" />
          配置已自动保存
        </>
      )}
    </div>
  );

  return (
    <>
      {noticeModalContextHolder}
      <div className="of-config-surface flex h-full w-full flex-col gap-3 animate-in">
        {/* Top clean Basic vs Advanced mode bar */}
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-slate-200/80 bg-white px-4 py-2.5 shadow-xs">
          <Space wrap size={12}>
            <Segmented<ConfigViewMode>
              value={viewMode}
              onChange={setViewMode}
              options={[
                { value: "basic", label: "基础设置（推荐）" },
                { value: "advanced", label: "高级设置" },
              ]}
            />
            <Text type="secondary" className="hidden text-xs sm:inline">
              {viewMode === "basic"
                ? "一屏配齐求职意向、简历、防封档位与可选 AI，改完即可开跑"
                : "多求职方案、正则表达式、自定义 Prompt、浏览器与数据备份"}
            </Text>
          </Space>
          <Space size={8}>
            {viewMode === "basic" && (
              <>
                <Button
                  size="small"
                  type="text"
                  icon={<DatabaseOutlined />}
                  onClick={() => {
                    setActiveGroup("data");
                    setViewMode("advanced");
                  }}
                >
                  备份
                </Button>
                <Button
                  size="small"
                  type="text"
                  icon={<InfoCircleOutlined />}
                  onClick={() => {
                    setActiveGroup("about");
                    setViewMode("advanced");
                  }}
                >
                  关于
                </Button>
              </>
            )}
            {saveStatusBadge}
          </Space>
        </div>

        {viewMode === "basic" ? (
          <div className="min-h-0 flex-1 overflow-y-auto pr-1">
            <BasicSettingsView
              config={props.config}
              dirty={props.dirty}
              onSelectResumePdf={selectResumePdf}
              updateJobFilter={props.updateJobFilter}
              updateResume={props.updateResume}
              updateGreet={props.updateGreet}
              updateGreetDefaultResource={props.updateGreetDefaultResource}
              addGreetDefaultResource={props.addGreetDefaultResource}
              updateReplay={props.updateReplay}
              updatePeriodicDelivery={props.updatePeriodicDelivery}
              updateHumanize={props.updateHumanize}
              updateLlm={props.updateLlm}
              persistLlm={props.persistLlm}
              persistConfig={props.persistConfig}
              updateLlmEnabled={props.updateLlmEnabled}
              onSwitchToAdvanced={(group) => {
                if (group) setActiveGroup(toVisibleConfigGroup(group));
                setViewMode("advanced");
              }}
            />
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-4 md:flex-row">
            <aside className="flex w-full flex-shrink-0 flex-col gap-3 md:w-[216px]">
              <Menu
                mode="vertical"
                selectedKeys={[
                  isProfileGroup(activeGroup) ? "profile" : activeGroup,
                ]}
                onSelect={({ key }) =>
                  setActiveGroup(
                    key === "profile"
                      ? isProfileGroup(activeGroup)
                        ? activeGroup
                        : "job"
                      : (key as VisibleConfigGroup),
                  )
                }
                items={menuItems}
                className="rounded-2xl border border-[var(--of-border)] bg-[var(--of-surface)]! p-2 shadow-xs"
              />
              <Dropdown
                menu={{
                  items: [
                    {
                      key: "import",
                      icon: <UploadOutlined />,
                      label: "导入配置模板",
                      onClick: selectConfigFile,
                    },
                    {
                      key: "export",
                      icon: <DownloadOutlined />,
                      label: "导出配置模板",
                      onClick: selectExportConfigFile,
                    },
                  ],
                }}
                trigger={["click"]}
              >
                <Button
                  disabled={props.status === "loading"}
                  className="h-10! w-full justify-start! text-slate-600!"
                >
                  配置文件 <MoreOutlined className="ml-auto" />
                </Button>
              </Dropdown>
              <Button
                type="text"
                size="small"
                icon={<SlidersOutlined />}
                onClick={() => setViewMode("basic")}
              >
                返回基础设置
              </Button>
            </aside>

            <Form
              form={form}
              layout="vertical"
              requiredMark={false}
              className="flex h-full min-w-0 flex-1 flex-col overflow-hidden"
              onSubmitCapture={(e) => {
                e.preventDefault();
              }}
            >
              <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-slate-200/80 bg-white shadow-xs">
                {isProfileGroup(activeGroup) && (
                  <div className="flex-shrink-0 border-b border-slate-200 bg-white px-5 pt-5 md:px-8 md:pt-6">
                    <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div className="min-w-0">
                        <Text className="mb-1 block text-[11px] font-bold uppercase tracking-[0.18em] text-sky-600">
                          当前求职方案
                        </Text>
                        <div className="flex flex-wrap items-center gap-2">
                          <Select
                            aria-label="当前求职方案"
                            size="large"
                            popupMatchSelectWidth={false}
                            className="min-w-[240px] max-w-full md:min-w-[320px]"
                            value={activeProfile.id}
                            onChange={props.onSelectProfile}
                            options={profiles.map((profile) => ({
                              value: profile.id,
                              label: `${profile.name}${profile.id === defaultProfileId ? " · 默认" : ""}${profile.archived ? " · 已归档" : ""}`,
                            }))}
                          />
                          {activeProfile.id === defaultProfileId && (
                            <Tag color="blue">默认使用</Tag>
                          )}
                          {activeProfile.archived && <Tag>已归档</Tag>}
                        </div>
                        <Text
                          type="secondary"
                          className="mt-2 block max-w-2xl text-xs!"
                        >
                          {activeProfile.description ||
                            "这张方案统一管理岗位筛选、简历、打招呼和自动回复。"}
                        </Text>
                      </div>
                      <Space wrap size={8}>
                        <Button
                          type="primary"
                          icon={<PlusOutlined />}
                          onClick={() => openProfileModal("create")}
                        >
                          新建方案
                        </Button>
                        <Dropdown
                          trigger={["click"]}
                          menu={{
                            onClick: handleProfileAction,
                            items: [
                              {
                                key: "edit",
                                icon: <EditOutlined />,
                                label: "编辑名称与说明",
                              },
                              {
                                key: "duplicate",
                                icon: <CopyOutlined />,
                                label: "复制当前方案",
                              },
                              { type: "divider" },
                              {
                                key: "default",
                                icon: <StarOutlined />,
                                label:
                                  activeProfile.id === defaultProfileId
                                    ? "当前已是默认方案"
                                    : "设为默认方案",
                                disabled:
                                  activeProfile.id === defaultProfileId ||
                                  activeProfile.archived,
                              },
                              {
                                key: "archive",
                                icon: <InboxOutlined />,
                                label: "归档当前方案",
                                disabled: !canArchive,
                              },
                              {
                                key: "delete",
                                icon: <DeleteOutlined />,
                                label: "删除当前方案",
                                danger: true,
                                disabled: !canDelete,
                              },
                            ],
                          }}
                        >
                          <Button icon={<MoreOutlined />}>方案管理</Button>
                        </Dropdown>
                      </Space>
                    </div>
                    <Tabs
                      className="mt-4 [&_.ant-tabs-nav]:mb-0!"
                      activeKey={activeGroup}
                      onChange={(key) => setActiveGroup(key as ProfileGroup)}
                      items={profileTabItems}
                    />
                  </div>
                )}
                <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-6 py-6 md:px-10 md:py-8">
                  {renderContent()}
                </div>
              </div>
            </Form>
          </div>
        )}
        <Modal
          title={
            profileModalMode === "create" ? "新建求职方案" : "编辑方案信息"
          }
          open={profileModalMode !== null}
          okText={profileModalMode === "create" ? "创建并切换" : "保存"}
          cancelText="取消"
          onCancel={() => setProfileModalMode(null)}
          onOk={saveProfileMeta}
          destroyOnHidden
        >
          <Space direction="vertical" size={14} className="mt-3 w-full">
            <div className="w-full">
              <Text strong>方案名称</Text>
              <Input
                autoFocus
                className="mt-2"
                maxLength={40}
                placeholder="例如：新加坡 AI 应用工程师"
                value={profileDraft.name}
                onChange={(event) =>
                  setProfileDraft((draft) => ({
                    ...draft,
                    name: event.target.value,
                  }))
                }
                onPressEnter={saveProfileMeta}
              />
            </div>
            <div className="w-full">
              <Text strong>方案说明</Text>
              <Input.TextArea
                className="mt-2"
                autoSize={{ minRows: 3, maxRows: 5 }}
                maxLength={160}
                showCount
                placeholder="说明目标岗位、地区或这张方案的使用场景"
                value={profileDraft.description}
                onChange={(event) =>
                  setProfileDraft((draft) => ({
                    ...draft,
                    description: event.target.value,
                  }))
                }
              />
            </div>
            {profileModalMode === "create" && (
              <Alert
                type="info"
                showIcon
                message="新方案会复制默认方案的当前配置，你可以创建后分别调整。"
              />
            )}
          </Space>
        </Modal>
      </div>
    </>
  );
}
