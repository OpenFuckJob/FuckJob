import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Button,
  Card,
  Cascader,
  Collapse,
  Image,
  Input,
  Space,
  Steps,
  Tag,
  Typography,
} from "antd";
import {
  CheckCircleOutlined,
  FilePdfOutlined,
  LoadingOutlined,
} from "@ant-design/icons";
import { IconFilterSlash, IconShieldCheck, OfferFlowLogo } from "@/components/icons";
import {
  getDefaultJobProfile,
  getJobProfiles,
  isLlmServiceUsable,
  type AppRuntimeConfig,
  type JobFilterConfig,
  type ResumeConfig,
} from "@/types/app-config";
import type { BrowserEnvStatus, EnvCheckResult } from "@/types/rpa";
import type { CommandResult } from "@/types/command";
import { BLACKLIST_PRESETS, mergeUniqueKeywords } from "@/lib/blacklist-presets";
import { cityTreeOptions, findCityPath } from "@/lib/constants";
import { setLlmApiKey } from "@/lib/llmConfig";
import {
  buildSafetyPresetPatch,
  detectSafetyPreset,
  SAFETY_PRESETS,
  type SafetyPresetId,
} from "@/lib/safety-presets";
import { NumberField } from "@/components/NumberField";
import { isValidLlmConfig, LlmConfigPanel } from "@/view/config/LlmConfigPanel";

function applyProfilePatch(
  draft: AppRuntimeConfig,
  patch: {
    jobFilter?: Partial<JobFilterConfig>;
    resume?: Partial<ResumeConfig>;
  },
): AppRuntimeConfig {
  const nextTopFilter: JobFilterConfig | undefined = draft.job_filter_config
    ? { ...draft.job_filter_config, ...(patch.jobFilter ?? {}) }
    : undefined;
  const nextTopResume: ResumeConfig | undefined = draft.resume_config
    ? { ...draft.resume_config, ...(patch.resume ?? {}) }
    : undefined;

  if (!draft.job_profiles?.length) {
    return {
      ...draft,
      ...(nextTopFilter ? { job_filter_config: nextTopFilter } : {}),
      ...(nextTopResume ? { resume_config: nextTopResume } : {}),
    };
  }

  const defaultId = getDefaultJobProfile(draft).id;
  const nextProfiles = getJobProfiles(draft).map((profile) =>
    profile.id === defaultId
      ? {
          ...profile,
          job_filter_config: { ...profile.job_filter_config, ...(patch.jobFilter ?? {}) },
          resume_config: { ...profile.resume_config, ...(patch.resume ?? {}) },
        }
      : profile,
  );
  return {
    ...draft,
    ...(nextTopFilter ? { job_filter_config: nextTopFilter } : {}),
    ...(nextTopResume ? { resume_config: nextTopResume } : {}),
    job_profiles: nextProfiles,
  };
}

export function Onboarding({
  config,
  onFinish,
}: {
  config: AppRuntimeConfig;
  onFinish: (config: AppRuntimeConfig) => Promise<boolean>;
}) {
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState(config);
  const [selectedSafety, setSelectedSafety] = useState<SafetyPresetId>(() => {
    const detected = detectSafetyPreset(config);
    return detected === "custom" ? "standard" : detected;
  });
  const [selectedBlacklistIds, setSelectedBlacklistIds] = useState<string[]>([
    "it_outsourcing",
    "outsourcing_terms",
  ]);
  const [browser, setBrowser] = useState<BrowserEnvStatus | null>(null);
  const [browserError, setBrowserError] = useState("");
  const [bossCheckLoading, setBossCheckLoading] = useState(false);
  const [bossEnv, setBossEnv] = useState<EnvCheckResult | null>(null);
  const [bossCheckError, setBossCheckError] = useState("");
  const [parsingResume, setParsingResume] = useState(false);
  const [resumeError, setResumeError] = useState("");
  const [pendingApiKey, setPendingApiKey] = useState("");
  const [finishing, setFinishing] = useState(false);
  const [finishError, setFinishError] = useState("");

  const activeFilter: Partial<JobFilterConfig> =
    (draft.job_profiles?.length ? getDefaultJobProfile(draft).job_filter_config : draft.job_filter_config) ??
    {};
  const activeResume: Partial<ResumeConfig> =
    (draft.job_profiles?.length ? getDefaultJobProfile(draft).resume_config : draft.resume_config) ??
    {};

  useEffect(() => {
    if (step !== 1) return;
    invoke<CommandResult<BrowserEnvStatus>>("check_browser_env")
      .then((result) => {
        if (result.success && result.data) setBrowser(result.data);
        else setBrowserError(result.error?.message ?? "浏览器检测失败");
      })
      .catch(() => setBrowserError("浏览器检测失败，可稍后在求职设置中处理"));
  }, [step]);

  const handleSelectResume = async () => {
    setResumeError("");
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "PDF 简历", extensions: ["pdf"] }],
    });
    if (typeof selected !== "string") return;

    setParsingResume(true);
    try {
      const result = await invoke<CommandResult<string>>("parse_resume_pdf", { path: selected });
      if (!result.success || result.data === null) {
        setResumeError(result.error?.message ?? "简历解析失败，可稍后在设置里重新上传");
        return;
      }
      setDraft((prev) =>
        applyProfilePatch(prev, {
          resume: {
            resume_path: selected,
            resume_content: result.data,
            inject_llm_context: true,
          },
        }),
      );
    } catch (error) {
      setResumeError(error instanceof Error ? error.message : "简历解析失败");
    } finally {
      setParsingResume(false);
    }
  };

  const handleCheckBossLogin = async () => {
    setBossCheckLoading(true);
    setBossCheckError("");
    try {
      const result = await invoke<CommandResult<EnvCheckResult>>("check_env", { platform: "boss" });
      if (!result.success || !result.data) {
        setBossCheckError(result.error?.message ?? "检查 BOSS 登录状态失败");
        return;
      }
      setBossEnv(result.data);
    } catch (error) {
      setBossCheckError(error instanceof Error ? error.message : "检查 BOSS 登录状态失败");
    } finally {
      setBossCheckLoading(false);
    }
  };

  const toggleBlacklistPreset = (id: string) => {
    setSelectedBlacklistIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id],
    );
  };

  const finish = async (options?: { skipAi?: boolean; applyPresets?: boolean }) => {
    if (finishing) return;
    setFinishing(true);
    setFinishError("");
    try {
      const llm = options?.skipAi ? null : draft.llm_config;
      if (llm && pendingApiKey.trim()) {
        const credentialResult = await setLlmApiKey(pendingApiKey.trim());
        if (!credentialResult.success) {
          setFinishError(credentialResult.error?.message ?? "API Key 保存失败");
          return;
        }
        setPendingApiKey("");
      }

      let finalDraft = draft;
      if (options?.applyPresets !== false) {
        const safetyPatch = buildSafetyPresetPatch(selectedSafety, finalDraft);
        const currentExclude = activeFilter.exclude_keywords ?? [];
        const currentCompanyExclude = activeFilter.company_exclude_keywords ?? [];
        let nextExclude = [...currentExclude];
        let nextCompanyExclude = [...currentCompanyExclude];
        for (const preset of BLACKLIST_PRESETS) {
          if (!selectedBlacklistIds.includes(preset.id)) continue;
          if (preset.target === "exclude_keywords") {
            nextExclude = mergeUniqueKeywords(nextExclude, preset.keywords);
          } else {
            nextCompanyExclude = mergeUniqueKeywords(nextCompanyExclude, preset.keywords);
          }
        }
        finalDraft = applyProfilePatch(
          {
            ...finalDraft,
            periodic_delivery_config: safetyPatch.periodic_delivery_config,
            humanize_config: safetyPatch.humanize_config,
          },
          {
            jobFilter: {
              exclude_keywords: nextExclude,
              company_exclude_keywords: nextCompanyExclude,
              max_greets_per_company: safetyPatch.max_greets_per_company,
            },
          },
        );
      }

      const saved = await onFinish({
        ...finalDraft,
        onboarding_completed: true,
        llm_config: llm,
        llm_fallbacks:
          llm === null
            ? []
            : (finalDraft.llm_fallbacks ?? []).filter(isLlmServiceUsable),
      });
      if (!saved) setFinishError("配置保存失败，请检查配置后重试");
    } catch (error) {
      setFinishError(error instanceof Error ? error.message : "进入应用失败，请重试");
    } finally {
      setFinishing(false);
    }
  };

  const llmDraftIncomplete = draft.llm_config !== null && !isValidLlmConfig(draft.llm_config);
  const bossQrSrc = bossEnv?.qr_code_base64
    ? `data:image/png;base64,${bossEnv.qr_code_base64}`
    : null;

  return (
    <main className="app-loading">
      <Card style={{ width: 760, maxWidth: "94vw" }}>
        <Space direction="vertical" size={18} style={{ width: "100%" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <OfferFlowLogo size={44} className="app-brand-logo" />
            <div>
              <Typography.Title level={3} style={{ margin: 0, letterSpacing: "-0.02em" }}>
                欢迎使用 OfferFlow · 3 步开启自动投递
              </Typography.Title>
              <Typography.Text type="secondary" style={{ fontSize: 13 }}>
                本地运行的自动投递与 HR 沟通管家：所有数据仅存于本机，不上传任何第三方服务器。
              </Typography.Text>
            </div>
          </div>

          <Steps
            size="small"
            current={step}
            items={[
              { title: "求职意向与简历" },
              { title: "浏览器与登录" },
              { title: "安全档位与 AI" },
            ]}
          />

          {step === 0 && (
            <Space direction="vertical" size={14} style={{ width: "100%" }}>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div>
                  <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                    目标岗位关键词
                  </Typography.Text>
                  <Input
                    placeholder="例如：前端开发、Java 后端、产品经理"
                    value={activeFilter.query ?? ""}
                    onChange={(event) =>
                      setDraft((prev) =>
                        applyProfilePatch(prev, {
                          jobFilter: { query: event.target.value || null },
                        }),
                      )
                    }
                  />
                </div>
                <div>
                  <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                    目标城市
                  </Typography.Text>
                  <Cascader
                    style={{ width: "100%" }}
                    options={cityTreeOptions}
                    placeholder="选择目标城市（默认全国）"
                    showSearch
                    expandTrigger="hover"
                    value={findCityPath(activeFilter.city)}
                    displayRender={(labels) => labels[labels.length - 1]}
                    onChange={(values) =>
                      setDraft((prev) =>
                        applyProfilePatch(prev, {
                          jobFilter: {
                            city: (values?.[values.length - 1] as number) || null,
                          },
                        }),
                      )
                    }
                  />
                </div>
              </div>

              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div>
                  <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                    最低月薪要求（K，可选）
                  </Typography.Text>
                  <NumberField
                    min={0}
                    max={300}
                    precision={0}
                    fallback={0}
                    style={{ width: "100%" }}
                    placeholder="例如：18（0 表示不限）"
                    value={activeFilter.min_salary_k ?? 0}
                    onChange={(value) =>
                      setDraft((prev) =>
                        applyProfilePatch(prev, {
                          jobFilter: { min_salary_k: value > 0 ? value : null },
                        }),
                      )
                    }
                  />
                </div>
                <div>
                  <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                    PDF 简历（可选，随时可补）
                  </Typography.Text>
                  <Space>
                    <Button
                      icon={parsingResume ? <LoadingOutlined /> : <FilePdfOutlined />}
                      loading={parsingResume}
                      onClick={() => void handleSelectResume()}
                    >
                      {activeResume.resume_content ? "重新选择 PDF" : "上传 PDF 简历"}
                    </Button>
                    {activeResume.resume_content && (
                      <Tag color="success" icon={<CheckCircleOutlined />}>
                        已解析 {activeResume.resume_content.length} 字
                      </Tag>
                    )}
                  </Space>
                </div>
              </div>
              {resumeError && <Alert type="warning" showIcon message={resumeError} />}

              <div>
                <Typography.Text strong style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
                  <IconFilterSlash size={15} style={{ color: "#f43f5e" }} />
                  <span>自动屏蔽水岗（默认推荐勾前两项）</span>
                </Typography.Text>
                <Space wrap size={8}>
                  {BLACKLIST_PRESETS.map((preset) => {
                    const active = selectedBlacklistIds.includes(preset.id);
                    return (
                      <Button
                        key={preset.id}
                        size="small"
                        type={active ? "primary" : "default"}
                        ghost={active}
                        onClick={() => toggleBlacklistPreset(preset.id)}
                      >
                        {active ? "✓ " : "+ "}
                        {preset.label}
                      </Button>
                    );
                  })}
                </Space>
              </div>

              <div style={{ display: "flex", justifyContent: "space-between", paddingTop: 6 }}>
                <Button loading={finishing} onClick={() => void finish({ skipAi: true, applyPresets: false })}>
                  先跳过，直接进入
                </Button>
                <Button type="primary" onClick={() => setStep(1)}>
                  下一步
                </Button>
              </div>
            </Space>
          )}

          {step === 1 && (
            <Space direction="vertical" size={14} style={{ width: "100%" }}>
              {browser ? (
                <Alert
                  type={browser.browser_found && browser.user_data_dir_ok ? "success" : "warning"}
                  showIcon
                  message={
                    browser.browser_found
                      ? `已检测到 ${browser.browser_name ?? "浏览器"}`
                      : "未自动检测到 Chrome 或 Edge"
                  }
                  description={browser.browser_path ?? "可稍后在「求职设置 · 高级设置」中手动指定路径"}
                />
              ) : browserError ? (
                <Alert type="warning" showIcon message={browserError} />
              ) : (
                <Typography.Text type="secondary">正在检测本机浏览器环境…</Typography.Text>
              )}

              <Card size="small">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12 }}>
                  <div>
                    <Typography.Text strong style={{ display: "block" }}>
                      现在检查 BOSS 直聘登录态（可选）
                    </Typography.Text>
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      提前扫码登录，进入首页就能直接点「开始自动投递」。
                    </Typography.Text>
                  </div>
                  <Button
                    loading={bossCheckLoading}
                    icon={bossCheckLoading ? <LoadingOutlined /> : <CheckCircleOutlined />}
                    onClick={() => void handleCheckBossLogin()}
                  >
                    检查 BOSS 登录
                  </Button>
                </div>
                {bossCheckError && (
                  <Alert style={{ marginTop: 10 }} type="warning" showIcon message={bossCheckError} />
                )}
                {bossEnv && (
                  <Alert
                    style={{ marginTop: 10 }}
                    type={bossEnv.status === "completed" ? "success" : "info"}
                    showIcon
                    message={bossEnv.message}
                  />
                )}
                {bossQrSrc && (
                  <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 10 }}>
                    <Image src={bossQrSrc} width={108} height={108} preview={false} alt="BOSS 直聘登录二维码" />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      请使用 BOSS 直聘 App 扫码确认登录，完成后即可点击下一步。
                    </Typography.Text>
                  </div>
                )}
              </Card>

              <div style={{ display: "flex", justifyContent: "space-between", paddingTop: 6 }}>
                <Button onClick={() => setStep(0)}>上一步</Button>
                <Space>
                  <Button loading={finishing} onClick={() => void finish({ skipAi: true })}>
                    跳过，直接进入
                  </Button>
                  <Button type="primary" onClick={() => setStep(2)}>
                    下一步
                  </Button>
                </Space>
              </div>
            </Space>
          )}

          {step === 2 && (
            <Space direction="vertical" size={14} style={{ width: "100%" }}>
              <div>
                <Typography.Text strong style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
                  <IconShieldCheck size={15} style={{ color: "var(--of-accent)" }} />
                  <span>选择防封安全档位（后续随时可改）</span>
                </Typography.Text>
                <div className="of-preset-grid">
                  {SAFETY_PRESETS.map((preset) => {
                    const selected = selectedSafety === preset.id;
                    return (
                      <button
                        key={preset.id}
                        type="button"
                        className={`of-preset-card${selected ? " is-selected" : ""}`}
                        onClick={() => setSelectedSafety(preset.id)}
                      >
                        <div className="of-preset-head">
                          <span className="of-preset-title">{preset.label}</span>
                          {preset.badge && <Tag color="blue" style={{ margin: 0 }}>{preset.badge}</Tag>}
                        </div>
                        <span className="of-preset-tagline">{preset.tagline}</span>
                        <span className="of-preset-summary">{preset.summary}</span>
                      </button>
                    );
                  })}
                </div>
              </div>

              <Collapse
                ghost
                defaultActiveKey={draft.llm_config ? ["ai"] : []}
                items={[
                  {
                    key: "ai",
                    label: (
                      <Typography.Text strong>
                        配置 AI 大模型（可选 · 不配也能按关键词 + 固定话术直接跑）
                      </Typography.Text>
                    ),
                    children: (
                      <LlmConfigPanel
                        config={draft.llm_config}
                        onChange={(llm_config) => setDraft((v) => ({ ...v, llm_config }))}
                        onPersist={async (llm_config) =>
                          onFinish({
                            ...draft,
                            onboarding_completed: false,
                            llm_config,
                            llm_fallbacks: (draft.llm_fallbacks ?? []).filter(isLlmServiceUsable),
                          })
                        }
                        onPendingApiKeyChange={setPendingApiKey}
                        compact
                      />
                    ),
                  },
                ]}
              />

              <div style={{ display: "flex", justifyContent: "space-between", paddingTop: 6 }}>
                <Button onClick={() => setStep(1)}>上一步</Button>
                <Space>
                  {llmDraftIncomplete && (
                    <Button loading={finishing} onClick={() => void finish({ skipAi: true })}>
                      暂不配 AI，直接进入
                    </Button>
                  )}
                  <Button
                    type="primary"
                    loading={finishing}
                    disabled={llmDraftIncomplete}
                    onClick={() => void finish()}
                  >
                    完成并开始使用
                  </Button>
                </Space>
              </div>
            </Space>
          )}

          {finishError && <Alert type="error" showIcon message={finishError} />}
        </Space>
      </Card>
    </main>
  );
}
