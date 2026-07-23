import {
  Form,
  Input,
  InputNumber,
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
  message as antdMessage,
} from "antd";
import {
  AppRuntimeConfig,
  ConfigGroup,
  StatusKind,
  MatchTarget,
  RuleMode,
  JobFilterConfig,
  GreetConfig,
  ReplayConfig,
  ReplyResource,
  ReplyTemplate,
  BrowserConfig,
  ResumeConfig,
  RegexRule,
} from "@/types/app-config";
import {
  jobTypeOptions,
  salaryOptions,
  experienceOptions,
  degreeOptions,
  industryTreeOptions,
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
} from "@ant-design/icons";
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commandErrorMessage, type CommandResult } from "@/types/command";
import type { BrowserEnvStatus } from "@/types/rpa";
import { MockInterviewDrawer } from "@/view/resume-optimizer/MockInterviewDrawer";
import {
  extractSections,
  findSectionIndexByRenderedTitle,
  replaceSectionContent,
} from "@/view/resume-optimizer";
import { LlmConfigPanel } from "./LlmConfigPanel";
import { AiFeatureGate } from "@/components/AiFeatureGate";

const { Title, Text } = Typography;

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
  ...buildCommonPromptVariables(resumeEnabled),
  {
    token: "{{background_context}}",
    description: "背景上下文，补充简历里没有体现的信息",
  },
  {
    token: "{{chat_history}}",
    description: "岗位沟通上下文",
  },
];

interface PromptVariableGuideProps {
  items: typeof basePromptVariableItems;
}

function PromptVariableGuide({ items }: PromptVariableGuideProps) {
  return (
    <div className="mt-3 rounded-2xl border border-sky-200 bg-sky-50 p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.7)]">
      <Text className="mb-3 block text-[10px] font-black uppercase tracking-[0.22em] text-sky-700">
        可用变量
      </Text>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        {items.map((item) => (
          <div
            key={item.token}
            className="rounded-xl border border-slate-200/80 bg-white px-3 py-2"
          >
            <code className="font-mono text-[11px] font-bold text-emerald-600">
              {item.token}
            </code>
            <Text className="mt-1 block text-[11px] leading-5 text-slate-500">
              {item.description}
            </Text>
          </div>
        ))}
      </div>
    </div>
  );
}

export interface ConfigPageProps {
  config: AppRuntimeConfig;
  status: StatusKind;
  message: string;
  dirty?: boolean;
  initialGroup?: ConfigGroup;
  onOpenLlmConfig: () => void;
  updateLlm: (next: AppRuntimeConfig["llm_config"]) => void;
  persistLlm: (next: AppRuntimeConfig["llm_config"]) => Promise<boolean>;
  updateJobFilter: (next: Partial<JobFilterConfig>) => void;
  updateGreet: (next: Partial<GreetConfig>) => void;
  updateGreetDefaultResource: (
    resourceIndex: number,
    next: Partial<ReplyResource>,
  ) => void;
  addGreetDefaultResource: () => void;
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
  updateBrowser: (next: Partial<BrowserConfig>) => void;
  updateResume: (next: Partial<ResumeConfig>) => void;
  updateRule: (index: number, next: Partial<RegexRule>) => void;
  addRule: () => void;
  addRules: (rules: RegexRule[]) => void;
  removeRule: (index: number) => void;
  importConfig: (path: string) => Promise<void>;
  exportConfig: (path: string) => Promise<void>;
}

const configGroupKeys = [
  "browser",
  "llm",
  "job",
  "resume",
  "greet",
  "reply",
] as const;
type VisibleConfigGroup = (typeof configGroupKeys)[number];

const toVisibleConfigGroup = (group?: ConfigGroup): VisibleConfigGroup =>
  configGroupKeys.includes(group as VisibleConfigGroup)
    ? (group as VisibleConfigGroup)
    : "resume";

const menuItems = [
  { type: "group" as const, label: "基础环境", children: [
    { key: "browser", icon: <GlobalOutlined />, label: "浏览器环境" },
    { key: "llm", icon: <RobotOutlined />, label: "大模型" },
  ] },
  { type: "group" as const, label: "求职策略", children: [
    { key: "job", icon: <ProductOutlined />, label: "岗位筛选" },
    { key: "resume", icon: <FilePdfOutlined />, label: "简历配置" },
  ] },
  { type: "group" as const, label: "沟通策略", children: [
    { key: "greet", icon: <CommentOutlined />, label: "打招呼配置" },
    { key: "reply", icon: <CommentOutlined />, label: "自动回复" },
  ] },
];

export function ConfigPage(props: ConfigPageProps) {
  const [activeGroup, setActiveGroup] = useState<VisibleConfigGroup>(() =>
    toVisibleConfigGroup(props.initialGroup),
  );
  const [form] = Form.useForm();
  const [browserEnvStatus, setBrowserEnvStatus] =
    useState<BrowserEnvStatus | null>(null);
  const [mockInterviewOpen, setMockInterviewOpen] = useState(false);
  const [ruleRequirement, setRuleRequirement] = useState("");
  const [generatingRules, setGeneratingRules] = useState(false);
  const resumeContent = props.config.resume_config.resume_content ?? "";
  const resumeSections = useMemo(
    () => extractSections(resumeContent),
    [resumeContent],
  );

  useEffect(() => {
    setActiveGroup(toVisibleConfigGroup(props.initialGroup));
  }, [props.initialGroup]);

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
      antdMessage.warning("请先描述岗位筛选需求");
      return;
    }
    if (!props.config.llm_config) {
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
      antdMessage.success(`已生成 ${result.data.length} 条规则，请检查后保存`);
    } catch (error) {
      antdMessage.error(
        error instanceof Error ? error.message : "生成高级规则失败",
      );
    } finally {
      setGeneratingRules(false);
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

  const resourceTypeOptions = (enableLlm: boolean) => [
    { value: "Text", label: "文本" },
    { value: "Image", label: "图片" },
    ...(enableLlm ? [{ value: "LLM", label: "LLM" }] : []),
  ];

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

    const loading = antdMessage.loading("正在解析 PDF 简历...", 0);
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
      antdMessage.success("简历解析完成");
    } catch (error: unknown) {
      antdMessage.error(
        error instanceof Error ? error.message : "简历解析失败",
      );
    } finally {
      loading();
    }
  };

  const applyMockInterviewOptimization = useCallback(
    async (sectionTitle: string, optimizedMarkdown: string) => {
      const sectionIndex = findSectionIndexByRenderedTitle(
        resumeSections,
        sectionTitle,
      );

      if (sectionIndex < 0) {
        throw new Error(`未找到章节「${sectionTitle}」`);
      }

      const nextContent = replaceSectionContent(
        resumeContent,
        resumeSections[sectionIndex],
        optimizedMarkdown,
      ).trim();

      form.setFieldsValue({
        resume_config: {
          ...props.config.resume_config,
          resume_content: nextContent,
        },
      });
      props.updateResume({ resume_content: nextContent });
      antdMessage.success("模拟面试优化已应用到简历配置");
    },
    [form, props, resumeContent, resumeSections],
  );

  // 当外部配置改变时（如首次加载），更新表单
  useEffect(() => {
    // 转换城市和行业代码为 Cascader 要求的路径格式
    const config = { ...props.config };
    const jobFilter = { ...config.job_filter_config };

    // 还原城市路径 [provinceCode, cityCode]
    if (jobFilter.city) {
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
      case "llm":
        return <LlmConfigPanel config={props.config.llm_config} onChange={props.updateLlm} onPersist={props.persistLlm} />;
      case "job":
        return (
          <Space direction="vertical" size="large" className="w-full">
            <div>
              <Title
                level={4}
                className="text-slate-900! m-0! flex items-center gap-2"
              >
                <ProductOutlined className="text-sky-500" />
                岗位筛选
              </Title>
              <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
                配置岗位检索、筛选及关键词策略
              </Text>
            </div>
            <Divider className="!my-0 opacity-10" />
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
                        className="border-sky-200! bg-sky-50/60!"
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
                              onChange={(event) => setRuleRequirement(event.target.value)}
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
                              {props.config.llm_config ? "生成规则" : "先配置大模型"}
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
                          className="rounded-lg! text-sky-600! hover:bg-sky-50! font-bold"
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
                              className="bg-white! hover:bg-slate-50! transition-colors border-slate-200/80 relative group/card"
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
            <Divider className="!my-0 opacity-10" />
            <AiFeatureGate configured={!!props.config.llm_config} onConfigure={props.onOpenLlmConfig}><></></AiFeatureGate>
            <div className="space-y-6">
              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
                <div className="flex items-center justify-between gap-6">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      LLM 主动沟通
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      启用后优先使用大模型生成打招呼内容
                    </Text>
                  </div>
                  <Form.Item
                    name={["greet_config", "enable_llm"]}
                    valuePropName="checked"
                    className="!m-0"
                  >
                    <Switch
                      disabled={!props.config.llm_config}
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
                    <PromptVariableGuide items={buildCommonPromptVariables(props.config.resume_config.inject_llm_context)} />
                  </div>
                )}
              </div>

              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-4">
                <div className="flex items-center justify-between">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      默认打招呼模板
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      未启用 LLM 时使用的兜底内容
                    </Text>
                  </div>
                  <Button
                    type="text"
                    icon={<PlusOutlined />}
                    onClick={props.addGreetDefaultResource}
                    className="text-sky-500! hover:bg-sky-50! rounded-lg! font-bold"
                  >
                    添加内容
                  </Button>
                </div>

                <div className="space-y-3">
                  {props.config.greet_config.default_template.map(
                    (resource, index) => (
                      <div
                        key={index}
                        className="grid grid-cols-1 md:grid-cols-[120px_1fr_40px] gap-3 items-start"
                      >
                        <Select
                          value={resource.resource_type}
                          onChange={(value) =>
                            props.updateGreetDefaultResource(index, {
                              resource_type:
                                value as ReplyResource["resource_type"],
                            })
                          }
                          options={resourceTypeOptions(
                            props.config.greet_config.enable_llm,
                          )}
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
                        <Button
                          type="text"
                          danger
                          icon={<DeleteOutlined />}
                          onClick={() =>
                            props.removeGreetDefaultResource(index)
                          }
                        />
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
      case "reply":
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
            <Divider className="!my-0 opacity-10" />
            <AiFeatureGate configured={!!props.config.llm_config} onConfigure={props.onOpenLlmConfig}><></></AiFeatureGate>
            <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
              <div className="flex items-center justify-between gap-6">
                <div>
                  <Text className="text-slate-900 font-bold block">
                    LLM 自动回复
                  </Text>
                  <Text className="text-slate-500 text-xs">
                    启用后使用大模型根据上下文生成回复
                  </Text>
                </div>
                <Form.Item
                  name={["replay_config", "enable_llm"]}
                  valuePropName="checked"
                  className="!m-0"
                >
                  <Switch
                    disabled={!props.config.llm_config}
                    onChange={(v) => props.updateReplay({ enable_llm: v })}
                  />
                </Form.Item>
              </div>

              {props.config.replay_config.enable_llm && (
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
                  <PromptVariableGuide items={buildReplyPromptVariables(props.config.resume_config.inject_llm_context)} />

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
                </div>
              )}
            </div>

            <div className="space-y-6">
              <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
                <div className="flex items-center justify-between gap-6">
                  <div>
                    <Text className="text-slate-900 font-bold block">
                      自动回复模板
                    </Text>
                    <Text className="text-slate-500 text-xs">
                      启用后使用正则规则匹配 HR 消息并发送固定回复
                    </Text>
                  </div>
                  <Form.Item
                    name={["replay_config", "enable_auto_replay"]}
                    valuePropName="checked"
                    className="!m-0"
                  >
                    <Switch
                      aria-label="启用自动回复"
                      onChange={(v) =>
                        props.updateReplay({ enable_auto_replay: v })
                      }
                    />
                  </Form.Item>
                </div>

                {props.config.replay_config.enable_auto_replay && (
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
                        className="text-sky-500! hover:bg-sky-50! rounded-lg! font-bold"
                      >
                        新增模板
                      </Button>
                    </div>

                    {props.config.replay_config.templates.map(
                      (template, index) => (
                        <Card
                          key={index}
                          size="small"
                          className="bg-white! border-slate-200/80 relative overflow-hidden"
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
                                <InputNumber
                                  size="small"
                                  min={1}
                                  precision={0}
                                  value={template.regex_rule.limit}
                                  onChange={(value) =>
                                    props.updateReplyTemplate(index, {
                                      regex_rule: {
                                        ...template.regex_rule,
                                        limit: value ?? 1,
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
                                  className="rounded-lg! text-sky-600! hover:bg-sky-50! font-bold"
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
                                      value={resource.resource_type}
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
                                      options={resourceTypeOptions(
                                        props.config.replay_config.enable_llm,
                                      )}
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
                )}
              </div>
            </div>
          </Space>
        );
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
            <Divider className="!my-0 opacity-10" />

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
                        已检测到 <strong>{browserEnvStatus.browser_name}</strong>，路径：
                        <code className="text-xs bg-slate-100 px-1.5 py-0.5 rounded">
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
                          系统中未找到 Google Chrome 或 Microsoft Edge，请手动配置浏览器可执行文件路径。
                        </p>
                        <div className="rounded-lg bg-amber-50 p-3 border border-amber-200">
                          <Text className="text-xs font-bold block mb-1.5 text-amber-800">
                            <InfoCircleOutlined className="mr-1" />
                            如何获取浏览器路径：
                          </Text>
                          <Text className="text-xs block text-amber-700">
                            在浏览器地址栏输入 <code className="bg-amber-100 px-1 rounded">chrome://version</code> 或{" "}
                            <code className="bg-amber-100 px-1 rounded">edge://version</code>，找到「个人资料路径」或「可执行文件路径」，复制后粘贴到下方「浏览器可执行文件路径」中。
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
              <Button
                icon={<RobotOutlined />}
                disabled={!resumeContent.trim()}
                onClick={() => props.config.llm_config ? setMockInterviewOpen(true) : props.onOpenLlmConfig()}
              >
                模拟面试优化
              </Button>
            </div>
            <AiFeatureGate configured={!!props.config.llm_config} onConfigure={props.onOpenLlmConfig}><></></AiFeatureGate>
            <Divider className="!my-0 opacity-10" />
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
          </Space>
        );
    }
  };

  return (
    <div className="flex h-full w-full flex-col md:flex-row gap-6 animate-in">
      <aside className="w-full md:w-[260px] flex-shrink-0 flex flex-col gap-4">
        <div className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs text-slate-500">
          {props.status === "loading" ? (
            <><LoadingOutlined className="mr-2 text-sky-500" />正在自动保存…</>
          ) : props.status === "error" ? (
            <><WarningOutlined className="mr-2 text-red-500" />{props.message || "自动保存失败，修改后将重试"}</>
          ) : props.dirty ? (
            <><LoadingOutlined className="mr-2 text-sky-500" />等待自动保存…</>
          ) : (
            <><CheckCircleOutlined className="mr-2 text-emerald-500" />配置已自动保存</>
          )}
        </div>
        <Menu
          mode="vertical"
          selectedKeys={[activeGroup]}
          onSelect={({ key }) => setActiveGroup(key as VisibleConfigGroup)}
          items={menuItems}
          className="rounded-3xl p-2 bg-white/85! border border-slate-200/80 shadow-[0_18px_40px_rgba(15,23,42,0.06)]"
        />
        <Button
          icon={<UploadOutlined />}
          onClick={selectConfigFile}
          className="w-full rounded-xl! h-11! border-sky-200 bg-sky-50 font-bold text-sky-700! hover:border-sky-300! hover:text-sky-800!"
          disabled={props.status === "loading"}
        >
          导入配置模板
        </Button>

        <Button
          icon={<DownloadOutlined />}
          onClick={selectExportConfigFile}
          className="w-full rounded-xl! h-11! border-emerald-200 bg-emerald-50 font-bold text-emerald-700! hover:border-emerald-300! hover:text-emerald-800!"
          disabled={props.status === "loading"}
        >
          导出配置模板
        </Button>

      </aside>

      <Form
        form={form}
        layout="vertical"
        requiredMark={false}
        className="flex-1 min-w-0 h-full overflow-hidden flex flex-col"
        onSubmitCapture={(e) => {
          e.preventDefault();
        }}
      >
        <div className="flex-1 h-full overflow-y-auto overflow-x-hidden rounded-3xl border border-slate-200/80 bg-white/82 px-6 py-6 md:px-12 md:py-10 backdrop-blur-3xl shadow-[0_24px_60px_rgba(15,23,42,0.06)]">
          {renderContent()}
        </div>
      </Form>
      <MockInterviewDrawer
        open={mockInterviewOpen}
        aiConfigured={!!props.config.llm_config}
        onConfigureAi={props.onOpenLlmConfig}
        resumeContent={resumeContent}
        onClose={() => setMockInterviewOpen(false)}
        onApply={applyMockInterviewOptimization}
      />
    </div>
  );
}
