import { useState } from "react";
import {
  Button,
  Card,
  Cascader,
  Col,
  Collapse,
  Input,
  Row,
  Segmented,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import {
  CheckCircleOutlined,
  FilePdfOutlined,
} from "@ant-design/icons";
import {
  IconFilterSlash,
  IconResumeDoc,
  IconShieldCheck,
  IconSliders,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import {
  isLlmActive,
  type AppRuntimeConfig,
  type ConfigGroup,
  type GreetConfig,
  type GreetResource,
  type HumanizeConfig,
  type JobFilterConfig,
  type PeriodicDeliveryConfig,
  type ReplayConfig,
  type ResumeConfig,
} from "@/types/app-config";
import { BLACKLIST_PRESETS, mergeUniqueKeywords } from "@/lib/blacklist-presets";
import { cityTreeOptions, findCityPath } from "@/lib/constants";
import {
  buildSafetyPresetPatch,
  detectSafetyPreset,
  SAFETY_PRESETS,
  type SafetyPresetId,
} from "@/lib/safety-presets";
import { NumberField } from "@/components/NumberField";
import { LlmConfigPanel } from "./LlmConfigPanel";

const { Title, Text } = Typography;

type BasicReplyMode = "off" | "template_first" | "llm" | "template";

function getBasicReplyMode(replay: ReplayConfig): BasicReplyMode {
  if (replay.enable_llm && replay.enable_template_reply) return "template_first";
  if (replay.enable_llm) return "llm";
  if (replay.enable_template_reply) return "template";
  return "off";
}

export interface BasicSettingsViewProps {
  config: AppRuntimeConfig;
  dirty?: boolean;
  onSelectResumePdf: () => Promise<void>;
  updateJobFilter: (next: Partial<JobFilterConfig>) => void;
  updateResume: (next: Partial<ResumeConfig>) => void;
  updateGreet: (next: Partial<GreetConfig>) => void;
  updateGreetDefaultResource: (index: number, next: Partial<GreetResource>) => void;
  addGreetDefaultResource: (
    resourceType?: GreetResource["resource_type"],
    content?: string,
  ) => void;
  updateReplay: (next: Partial<ReplayConfig>) => void;
  updatePeriodicDelivery: (next: Partial<PeriodicDeliveryConfig>) => void;
  updateHumanize: (next: Partial<HumanizeConfig>) => void;
  updateLlm: (next: AppRuntimeConfig["llm_config"]) => void;
  persistLlm: (next: AppRuntimeConfig["llm_config"]) => Promise<boolean>;
  persistConfig: () => Promise<boolean>;
  updateLlmEnabled: (next: AppRuntimeConfig["llm_enabled"]) => void;
  onSwitchToAdvanced: (group?: ConfigGroup) => void;
}

export function BasicSettingsView({
  config,
  dirty,
  onSelectResumePdf,
  updateJobFilter,
  updateResume,
  updateGreet,
  updateGreetDefaultResource,
  addGreetDefaultResource,
  updateReplay,
  updatePeriodicDelivery,
  updateHumanize,
  updateLlm,
  persistLlm,
  persistConfig,
  updateLlmEnabled,
  onSwitchToAdvanced,
}: BasicSettingsViewProps) {
  const [messageApi, contextHolder] = message.useMessage();
  const [resumePreviewOpen, setResumePreviewOpen] = useState(false);

  const filter = config.job_filter_config;
  const resume = config.resume_config;
  const greet = config.greet_config;
  const replay = config.replay_config;
  const llmActive = isLlmActive(config);
  const llmEnabled = config.llm_enabled !== false;
  const activeSafety = detectSafetyPreset(config);

  const firstTextIndex = greet.default_template.findIndex(
    (item) => item.resource_type === "Text",
  );
  const defaultGreetText =
    firstTextIndex >= 0 ? greet.default_template[firstTextIndex].content : "";

  const handleDefaultGreetChange = (text: string) => {
    if (firstTextIndex >= 0) {
      updateGreetDefaultResource(firstTextIndex, { content: text, enabled: true });
    } else {
      addGreetDefaultResource("Text", text);
    }
  };

  const handleGreetModeChange = (mode: "static" | "llm") => {
    if (mode === "llm") {
      const hasLlm = greet.default_template.some((item) => item.resource_type === "LLM");
      const nextTemplate: GreetResource[] = greet.default_template.map((item) => {
        if (item.resource_type === "LLM") return { ...item, enabled: true };
        if (item.resource_type === "Text") return { ...item, enabled: false };
        return item;
      });
      if (!hasLlm) {
        nextTemplate.push({ resource_type: "LLM", content: "", enabled: true });
      }
      updateGreet({ enable_llm: true, default_template: nextTemplate });
      return;
    }

    const hasText = greet.default_template.some((item) => item.resource_type === "Text");
    let enabledFirstText = false;
    const nextTemplate: GreetResource[] = greet.default_template.map((item) => {
      if (item.resource_type === "LLM") return { ...item, enabled: false };
      if (item.resource_type === "Text" && !enabledFirstText) {
        enabledFirstText = true;
        return { ...item, enabled: true };
      }
      return item;
    });
    if (!hasText) {
      nextTemplate.push({ resource_type: "Text", content: "", enabled: true });
    }
    updateGreet({ enable_llm: false, default_template: nextTemplate });
  };

  const handleApplySafetyPreset = (presetId: SafetyPresetId) => {
    const patch = buildSafetyPresetPatch(presetId, config);
    updatePeriodicDelivery(patch.periodic_delivery_config);
    updateHumanize(patch.humanize_config);
    updateJobFilter({ max_greets_per_company: patch.max_greets_per_company });
    const label = SAFETY_PRESETS.find((item) => item.id === presetId)?.label ?? "防封档位";
    messageApi.success(`已切换为「${label}」`);
  };

  const handleReplyModeChange = (mode: BasicReplyMode) => {
    const flags: Record<BasicReplyMode, Pick<ReplayConfig, "enable_llm" | "enable_template_reply">> =
      {
        off: { enable_llm: false, enable_template_reply: false },
        template: { enable_llm: false, enable_template_reply: true },
        llm: { enable_llm: true, enable_template_reply: false },
        template_first: { enable_llm: true, enable_template_reply: true },
      };
    updateReplay(flags[mode]);
  };

  const resumeFileName = resume.resume_path
    ? resume.resume_path.split(/[\\/]/).pop()
    : null;

  return (
    <div className="mx-auto flex w-full max-w-[920px] flex-col gap-5 pb-6">
      {contextHolder}

      {/* 1. 求职意向 */}
      <Card id="basic-section-job" size="small" styles={{ body: { padding: 20 } }}>
        <div className="mb-4 flex items-center justify-between gap-2">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-500">
              <IconTarget size={17} />
            </span>
            <div>
              <Title level={5} className="m-0! tracking-tight">
                1. 求职意向（我要找什么工作）
              </Title>
              <Text type="secondary" className="text-xs">
                填好岗位关键词、城市和屏蔽词即可开始自动筛选，过滤掉外包与低薪水岗。
              </Text>
            </div>
          </div>
          <Button type="link" size="small" onClick={() => onSwitchToAdvanced("job")}>
            更多行业 / 学历 / 平台筛选 →
          </Button>
        </div>

        <Row gutter={[16, 14]}>
          <Col xs={24} md={10}>
            <Text strong className="mb-1.5 block text-xs">
              目标岗位关键词
            </Text>
            <Input
              placeholder="例如：前端开发、Java 后端、AI 应用工程师"
              value={filter.query ?? ""}
              onChange={(event) => updateJobFilter({ query: event.target.value || null })}
            />
          </Col>
          <Col xs={24} md={7}>
            <Text strong className="mb-1.5 block text-xs">
              目标城市
            </Text>
            <Cascader
              style={{ width: "100%" }}
              options={cityTreeOptions}
              placeholder="默认全国"
              showSearch
              expandTrigger="hover"
              value={findCityPath(filter.city)}
              displayRender={(labels) => labels[labels.length - 1]}
              onChange={(values) =>
                updateJobFilter({
                  city: (values?.[values.length - 1] as number) || null,
                })
              }
            />
          </Col>
          <Col xs={14} md={4}>
            <Text strong className="mb-1.5 block text-xs">
              最低月薪 (K)
            </Text>
            <NumberField
              min={0}
              max={300}
              precision={0}
              fallback={0}
              style={{ width: "100%" }}
              value={filter.min_salary_k ?? 0}
              onChange={(value) => updateJobFilter({ min_salary_k: value > 0 ? value : null })}
            />
          </Col>
          <Col xs={10} md={3}>
            <Text strong className="mb-1.5 block text-xs">
              排除面议
            </Text>
            <div className="pt-1">
              <Switch
                checked={Boolean(filter.exclude_negotiable)}
                onChange={(checked) => updateJobFilter({ exclude_negotiable: checked })}
              />
            </div>
          </Col>

          <Col xs={24} md={12}>
            <Text strong className="mb-1.5 block text-xs">
              岗位必须包含词（可选）
            </Text>
            <Select
              mode="tags"
              style={{ width: "100%" }}
              placeholder="输入后回车，例如：React、TypeScript"
              value={filter.keywords}
              onChange={(keywords) => updateJobFilter({ keywords })}
            />
          </Col>
          <Col xs={24} md={12}>
            <Text strong className="mb-1.5 block text-xs">
              岗位排除词（命中即跳过）
            </Text>
            <Select
              mode="tags"
              style={{ width: "100%" }}
              placeholder="输入后回车，例如：外包、驻场、销售"
              value={filter.exclude_keywords}
              onChange={(exclude_keywords) => updateJobFilter({ exclude_keywords })}
            />
          </Col>
          <Col xs={24}>
            <Text strong className="mb-1.5 block text-xs">
              屏蔽公司关键词（命中即跳过）
            </Text>
            <Select
              mode="tags"
              style={{ width: "100%" }}
              placeholder="输入公司名后回车，例如：软通动力、中软国际"
              value={filter.company_exclude_keywords}
              onChange={(company_exclude_keywords) =>
                updateJobFilter({ company_exclude_keywords })
              }
            />
          </Col>
        </Row>

        <div className="mt-4 flex flex-wrap items-center gap-2 rounded-xl border border-[var(--of-border)] bg-[var(--of-surface-subtle)] px-3.5 py-2.5">
          <span className="flex items-center gap-1.5 text-xs font-semibold text-[var(--of-text-secondary)]">
            <IconFilterSlash size={14} className="text-rose-500" />
            一键屏蔽水岗：
          </span>
          {BLACKLIST_PRESETS.map((preset) => (
            <Button
              key={preset.id}
              size="small"
              title={preset.description}
              onClick={() => {
                const existing = filter[preset.target] ?? [];
                const merged = mergeUniqueKeywords(existing, preset.keywords);
                const added = merged.length - existing.length;
                updateJobFilter({ [preset.target]: merged });
                messageApi.success(
                  added > 0
                    ? `已追加 ${added} 个「${preset.label}」过滤词`
                    : `「${preset.label}」过滤词已全部存在`,
                );
              }}
            >
              + {preset.label}
            </Button>
          ))}
        </div>
      </Card>

      {/* 2. 简历与打招呼 */}
      <Card id="basic-section-resume" size="small" styles={{ body: { padding: 20 } }}>
        <div className="mb-4 flex items-center justify-between gap-2">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-sky-500/10 text-sky-500">
              <IconResumeDoc size={17} />
            </span>
            <div>
              <Title level={5} className="m-0! tracking-tight">
                2. 简历与沟通方式
              </Title>
              <Text type="secondary" className="text-xs">
                上传 PDF 简历并决定如何跟 HR 打招呼与回复未读消息。
              </Text>
            </div>
          </div>
          <Button type="link" size="small" onClick={() => onSwitchToAdvanced("greet")}>
            编辑 Prompt 与正则模板 →
          </Button>
        </div>

        <Space direction="vertical" size={16} style={{ width: "100%" }}>
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[var(--of-border)] bg-[var(--of-surface-subtle)] px-4 py-3">
            <div className="flex items-center gap-3">
              <FilePdfOutlined className="text-xl text-rose-500" />
              <div>
                <Text strong className="block text-sm">
                  {resumeFileName || "尚未上传 PDF 简历"}
                </Text>
                <Text type="secondary" className="text-xs">
                  {resume.resume_content
                    ? `已解析 ${resume.resume_content.length} 字，将用于 AI 定制开场白与智能回复`
                    : "上传后自动解析正文，只保存在本机"}
                </Text>
              </div>
            </div>
            <Space>
              {resume.resume_content && (
                <Button size="small" onClick={() => setResumePreviewOpen((v) => !v)}>
                  {resumePreviewOpen ? "收起内容" : "查看 / 微调"}
                </Button>
              )}
              <Button
                type={resume.resume_content ? "default" : "primary"}
                size="small"
                icon={<FilePdfOutlined />}
                onClick={() => void onSelectResumePdf()}
              >
                {resume.resume_content ? "更换 PDF" : "选择 PDF 简历"}
              </Button>
            </Space>
          </div>

          {resumePreviewOpen && (
            <Input.TextArea
              rows={6}
              value={resume.resume_content ?? ""}
              placeholder="简历正文内容"
              onChange={(event) => updateResume({ resume_content: event.target.value || null })}
            />
          )}

          <Row gutter={[16, 14]} align="top">
            <Col xs={24} md={12}>
              <Text strong className="mb-1.5 block text-xs">
                打招呼方式
              </Text>
              <Segmented<"static" | "llm">
                block
                value={greet.enable_llm ? "llm" : "static"}
                options={[
                  { value: "static", label: "固定开场白（免 AI）" },
                  { value: "llm", label: "AI 结合简历定制" },
                ]}
                onChange={handleGreetModeChange}
              />
              {greet.enable_llm ? (
                <Text type="secondary" className="mt-2 block text-xs">
                  {llmActive
                    ? "已启用 AI 定制打招呼：系统会结合你的简历与岗位 JD 在前 17 字突出核心优势。"
                    : "已开启 AI 打招呼，请在下方「4. AI 大模型」完成配置，否则启动前会提醒。"}
                </Text>
              ) : (
                <div className="mt-2.5">
                  <Input.TextArea
                    rows={3}
                    value={defaultGreetText}
                    placeholder="例如：您好，我有 5 年前端架构与 React/TypeScript 经验，看过贵司 JD 匹配度很高，方便沟通一下吗？"
                    onChange={(event) => handleDefaultGreetChange(event.target.value)}
                  />
                </div>
              )}
            </Col>

            <Col xs={24} md={12}>
              <Text strong className="mb-1.5 block text-xs">
                收到 HR 未读消息时
              </Text>
              <Select<BasicReplyMode>
                style={{ width: "100%" }}
                value={getBasicReplyMode(replay)}
                onChange={handleReplyModeChange}
                options={[
                  { value: "template_first", label: "规则优先，AI 兜底（推荐）" },
                  { value: "llm", label: "全部交给 AI 智能回复" },
                  { value: "template", label: "仅回复命中固定规则的消息" },
                  { value: "off", label: "关闭自动回复（仅同步消息）" },
                ]}
              />
              <div className="mt-3 flex items-center justify-between rounded-lg border border-[var(--of-border)] px-3 py-2">
                <div>
                  <Text className="block text-xs font-medium">HR 要简历时自动发送附件</Text>
                  <Text type="secondary" className="text-[11px]">
                    关闭后只文字回复，发简历交给你手动确认
                  </Text>
                </div>
                <Switch
                  size="small"
                  checked={replay.enable_auto_send_resume}
                  onChange={(checked) => updateReplay({ enable_auto_send_resume: checked })}
                />
              </div>
            </Col>
          </Row>
        </Space>
      </Card>

      {/* 3. 安全与防封档位 */}
      <Card id="basic-section-runtime" size="small" styles={{ body: { padding: 20 } }}>
        <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-500">
              <IconShieldCheck size={17} />
            </span>
            <div>
              <Title level={5} className="m-0! tracking-tight">
                3. 防封安全档位
              </Title>
              <Text type="secondary" className="text-xs">
                一键打包每日投递上限、单轮条数、时段窗口与拟人化节奏，降低账号风控风险。
              </Text>
            </div>
          </div>
          <Space size={8}>
            {activeSafety === "custom" && <Tag color="blue">当前为自定义参数</Tag>}
            <Button type="link" size="small" onClick={() => onSwitchToAdvanced("runtime")}>
              精细调整时段 / 通知 →
            </Button>
          </Space>
        </div>

        <div className="of-preset-grid">
          {SAFETY_PRESETS.map((preset) => {
            const selected = activeSafety === preset.id;
            return (
              <button
                key={preset.id}
                type="button"
                className={`of-preset-card${selected ? " is-selected" : ""}`}
                onClick={() => handleApplySafetyPreset(preset.id)}
              >
                <div className="of-preset-head">
                  <span className="of-preset-title">{preset.label}</span>
                  {selected ? (
                    <Tag color="success" icon={<CheckCircleOutlined />} style={{ margin: 0 }}>
                      使用中
                    </Tag>
                  ) : preset.badge ? (
                    <Tag color="blue" style={{ margin: 0 }}>
                      {preset.badge}
                    </Tag>
                  ) : null}
                </div>
                <span className="of-preset-tagline">{preset.tagline}</span>
                <span className="of-preset-summary">{preset.summary}</span>
              </button>
            );
          })}
        </div>
      </Card>

      {/* 4. AI 大模型（可选） */}
      <Card id="basic-section-llm" size="small" styles={{ body: { padding: 20 } }}>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-violet-500/10 text-violet-500">
              <IconSparkles size={17} />
            </span>
            <div>
              <Title level={5} className="m-0! flex items-center gap-2 tracking-tight">
                4. AI 大模型（可选）
                {llmActive ? (
                  <Tag color="success" style={{ margin: 0 }}>
                    已就绪 · {config.llm_config?.model}
                  </Tag>
                ) : (
                  <Tag style={{ margin: 0 }}>未启用（纯规则可用）</Tag>
                )}
              </Title>
              <Text type="secondary" className="text-xs">
                配置后可解锁 AI 定制打招呼、自动回复 HR 与岗位匹配分析；API Key 仅保存在系统钥匙串。
              </Text>
            </div>
          </div>
          <Button type="link" size="small" onClick={() => onSwitchToAdvanced("llm")}>
            备用降级链与重试策略 →
          </Button>
        </div>

        <Collapse
          ghost
          defaultActiveKey={llmActive ? [] : ["llm-editor"]}
          items={[
            {
              key: "llm-editor",
              label: (
                <Text strong className="text-xs">
                  {llmActive ? "展开修改主用模型或 API Key" : "展开配置 AI 模型（支持 DeepSeek / OpenAI / Anthropic / Ollama 等）"}
                </Text>
              ),
              children: (
                <LlmConfigPanel
                  compact
                  config={config.llm_config}
                  enabled={llmEnabled}
                  onChange={updateLlm}
                  onEnabledChange={(enabled) => updateLlmEnabled(enabled ? undefined : false)}
                  onPersist={persistLlm}
                  onPersistAll={persistConfig}
                  dirty={dirty}
                />
              ),
            },
          ]}
        />
      </Card>

      {/* Subtle footer shortcut into Advanced Settings */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-dashed border-[var(--of-border-strong)] px-4 py-3">
        <Text type="secondary" className="text-xs">
          需要多求职方案、正则表达式、自定义 Prompt、浏览器路径、Webhook 通知或数据备份？
        </Text>
        <Button
          size="small"
          icon={<IconSliders size={14} />}
          onClick={() => onSwitchToAdvanced("job")}
        >
          进入高级设置
        </Button>
      </div>
    </div>
  );
}
