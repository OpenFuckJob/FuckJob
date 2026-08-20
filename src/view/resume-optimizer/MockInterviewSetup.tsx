import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Collapse,
  Input,
  Select,
  Space,
  Tag,
  Typography,
} from "antd";
import {
  ApartmentOutlined,
  BulbOutlined,
  ClockCircleOutlined,
  CodeOutlined,
  ExperimentOutlined,
  FileTextOutlined,
  ReloadOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { fetchJobDescriptionText } from "@/lib/job-description";
import type { CommandResult } from "@/types/command";
import type { JobDetail } from "@/types/job-detail";
import {
  DURATION_META,
  type InterviewDuration,
  type MockInterviewSettings,
} from "./interview-types";

export type { MockInterviewSettings } from "./interview-types";

const PLACEHOLDER_JOB_TITLES = new Set(["聊天同步岗位"]);
const PLACEHOLDER_COMPANY_NAMES = new Set(["BOSS 会话"]);
const FOCUS_OPTIONS = [
  "项目深挖",
  "个人贡献",
  "Agent工作流",
  "RAG与知识库",
  "系统设计",
  "工程稳定性",
  "性能与成本",
  "协作沟通",
];

/** 每种面试各配专属图标，一眼能区分，不靠读文字 */
const INTERVIEW_TYPES = [
  { value: "综合面", icon: <ApartmentOutlined />, description: "岗位、项目、专业和行为能力" },
  { value: "技术面", icon: <CodeOutlined />, description: "技术深度、设计和问题排查" },
  { value: "项目深挖", icon: <ExperimentOutlined />, description: "真实性、个人贡献和项目结果" },
];

/** 难度直接说明追问强度，避免只给一个不知深浅的档位名 */
const DIFFICULTY_OPTIONS = [
  { value: "初级", description: "偏基础概念与流程，节奏较缓" },
  { value: "中级", description: "结合项目追问细节与技术取舍" },
  { value: "高级", description: "深挖系统设计与边界，追问更紧" },
];

export function isSelectableInterviewJob(job: JobDetail): boolean {
  const title = job.title.trim();
  const company = job.company_name.trim();
  return title.length > 0 && !PLACEHOLDER_JOB_TITLES.has(title) && !PLACEHOLDER_COMPANY_NAMES.has(company);
}

/**
 * 拼给模型的岗位上下文。
 *
 * `description` 由调用方从 [`fetchJobDescriptionText`] 取——抓下来的
 * `job.detail` 混着反爬注入的样式代码和噪声词，原样喂进去既占额度又干扰判断。
 * 这里保持纯函数，取数留在调用方
 */
export function buildInterviewJobContext(job: JobDetail, description: string): string {
  const metadata = [
    `岗位：${job.title.trim()}`,
    job.company_name.trim() ? `公司：${job.company_name.trim()}` : "",
    job.salary.trim() ? `薪资：${job.salary.trim()}` : "",
    job.location?.trim() ? `地点：${job.location.trim()}` : "",
  ].filter(Boolean);
  const detail = description.trim();
  return [...metadata, detail ? `JD：\n${detail}` : ""].filter(Boolean).join("\n").slice(0, 6000);
}

interface MockInterviewSetupProps {
  value: MockInterviewSettings;
  onChange: (value: MockInterviewSettings) => void;
  disabled?: boolean;
}

export function MockInterviewSetup({ value, onChange, disabled }: MockInterviewSetupProps) {
  const [jobs, setJobs] = useState<JobDetail[]>([]);
  const [jobsLoading, setJobsLoading] = useState(false);
  const [jobsError, setJobsError] = useState("");
  const [customJobOpen, setCustomJobOpen] = useState(false);
  // 清洗后的 JD，展示和拼上下文共用同一份，不再各读一次 job.detail
  const [description, setDescription] = useState("");
  const patch = (next: Partial<MockInterviewSettings>) => onChange({ ...value, ...next });

  const loadJobs = () => {
    setJobsLoading(true);
    setJobsError("");
    void invoke<CommandResult<JobDetail[]>>("job_list")
      .then((result) => {
        if (!result.success || !result.data) {
          setJobsError(result.error?.message || "加载已抓取岗位失败");
          return;
        }
        setJobs(result.data.filter(isSelectableInterviewJob));
      })
      .catch((error: unknown) => {
        setJobsError(error instanceof Error ? error.message : "加载已抓取岗位失败");
      })
      .finally(() => setJobsLoading(false));
  };

  useEffect(loadJobs, []);

  const jobOptions = useMemo(
    () => [...jobs]
      .sort((left, right) => right.updated_at.localeCompare(left.updated_at))
      .map((job) => ({
        value: job.id,
        label: `${job.title} · ${job.company_name || "公司未知"}`,
      })),
    [jobs],
  );
  const selectedJob = jobs.find((item) => item.id === value.selectedJobId);

  const selectJob = async (jobId?: string) => {
    if (!jobId) {
      setDescription("");
      patch({ selectedJobId: undefined, jobTitle: "", companyName: "", jobContext: "" });
      return;
    }
    const job = jobs.find((item) => item.id === jobId);
    if (!job) return;
    setCustomJobOpen(false);
    const detail = await fetchJobDescriptionText(job.id);
    setDescription(detail);
    patch({
      selectedJobId: job.id,
      jobTitle: job.title.trim(),
      companyName: job.company_name.trim(),
      jobContext: buildInterviewJobContext(job, detail),
    });
  };

  const toggleFocus = (focus: string) => {
    if (value.focusAreas.includes(focus)) {
      patch({ focusAreas: value.focusAreas.filter((item) => item !== focus) });
      return;
    }
    if (value.focusAreas.length < 3) patch({ focusAreas: [...value.focusAreas, focus] });
  };

  return (
    <div className="mi-setup-form">
      <section className="mi-form-section">
        <div className="mi-section-heading">
          <div className="mi-section-number">1</div>
          <div>
            <Typography.Title level={5}>目标岗位</Typography.Title>
            <Typography.Text type="secondary">从已抓取岗位中选择，问题会结合真实JD生成</Typography.Text>
          </div>
        </div>
        <Select
          value={value.selectedJobId}
          disabled={disabled}
          loading={jobsLoading}
          allowClear
          showSearch
          suffixIcon={<SearchOutlined />}
          optionFilterProp="label"
          placeholder="搜索公司或岗位名称"
          notFoundContent={jobsLoading ? "正在加载岗位…" : "暂无可用的真实岗位数据"}
          options={jobOptions}
          onChange={(jobId) => void selectJob(jobId)}
          className="mi-job-select"
        />
        {jobsError && (
          <Alert
            type="warning"
            showIcon
            message={jobsError}
            action={<Button size="small" icon={<ReloadOutlined />} onClick={loadJobs}>重试</Button>}
          />
        )}
        {selectedJob && (
          <Card className="mi-selected-job" size="small">
            <div className="mi-job-card-title">
              <div>
                <Typography.Text strong>{selectedJob.title}</Typography.Text>
                <Typography.Text type="secondary">{selectedJob.company_name || "公司未知"}</Typography.Text>
              </div>
              <Space size={6} wrap>
                {selectedJob.location && <Tag>{selectedJob.location}</Tag>}
                {selectedJob.salary && <Tag color="blue">{selectedJob.salary}</Tag>}
              </Space>
            </div>
            {description && (
              <Collapse
                ghost
                size="small"
                items={[{ key: "jd", label: "查看完整岗位描述", children: <div className="mi-job-detail">{description}</div> }]}
              />
            )}
          </Card>
        )}
        <Button type="link" className="mi-custom-job-trigger" onClick={() => setCustomJobOpen((open) => !open)}>
          {customJobOpen ? "收起自定义岗位" : "没有合适岗位？使用自定义岗位"}
        </Button>
        {customJobOpen && (
          <div className="mi-custom-job-fields">
            <Input
              value={value.jobTitle}
              placeholder="岗位名称"
              onChange={(event) => patch({ selectedJobId: undefined, jobTitle: event.target.value })}
            />
            <Input
              value={value.companyName}
              placeholder="公司名称（选填）"
              onChange={(event) => patch({ selectedJobId: undefined, companyName: event.target.value })}
            />
            <Input.TextArea
              value={value.jobContext}
              placeholder="粘贴岗位职责与任职要求"
              autoSize={{ minRows: 4, maxRows: 8 }}
              maxLength={6000}
              onChange={(event) => patch({ selectedJobId: undefined, jobContext: event.target.value })}
            />
          </div>
        )}
      </section>

      <section className="mi-form-section">
        <div className="mi-section-heading">
          <div className="mi-section-number">2</div>
          <div>
            <Typography.Title level={5}>面试配置</Typography.Title>
            <Typography.Text type="secondary">选择面试方向、时长和岗位难度</Typography.Text>
          </div>
        </div>
        <Typography.Text className="mi-field-label">面试类型</Typography.Text>
        <div className="mi-choice-grid mi-choice-grid-three">
          {INTERVIEW_TYPES.map((option) => (
            <button
              type="button"
              key={option.value}
              className={`mi-choice-card ${value.interviewType === option.value ? "is-selected" : ""}`}
              onClick={() => patch({ interviewType: option.value })}
            >
              <span className="mi-choice-icon">{option.icon}</span>
              <strong>{option.value}</strong>
              <span>{option.description}</span>
            </button>
          ))}
        </div>

        <Typography.Text className="mi-field-label">面试时长</Typography.Text>
        <div className="mi-choice-grid mi-choice-grid-three">
          {(Object.entries(DURATION_META) as Array<[InterviewDuration, typeof DURATION_META[InterviewDuration]]>).map(([key, meta]) => (
            <button
              type="button"
              key={key}
              className={`mi-duration-card ${value.duration === key ? "is-selected" : ""}`}
              onClick={() => patch({ duration: key })}
            >
              <div className="mi-duration-head">
                <strong>{meta.label}</strong>
                <em>约 {meta.targetQuestions} 题</em>
              </div>
              <span>
                <ClockCircleOutlined /> {meta.minutes}
              </span>
            </button>
          ))}
        </div>

        <Typography.Text className="mi-field-label">难度</Typography.Text>
        <div className="mi-choice-grid mi-choice-grid-three">
          {DIFFICULTY_OPTIONS.map((option) => (
            <button
              type="button"
              key={option.value}
              className={`mi-duration-card ${value.difficulty === option.value ? "is-selected" : ""}`}
              onClick={() => patch({ difficulty: option.value })}
            >
              <div className="mi-duration-head">
                <strong>{option.value}</strong>
              </div>
              <span>{option.description}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="mi-form-section">
        <div className="mi-section-heading">
          <div className="mi-section-number">3</div>
          <div>
            <Typography.Title level={5}>面试侧重点 <Typography.Text type="secondary">（选填）</Typography.Text></Typography.Title>
            <Typography.Text type="secondary">不设置时，AI会根据岗位与简历自动规划</Typography.Text>
          </div>
        </div>
        <div className="mi-smart-plan-note">
          <BulbOutlined />
          <span>{value.focusAreas.length ? `已选择 ${value.focusAreas.length}/3 个侧重点` : "当前使用AI智能规划"}</span>
          {!!value.focusAreas.length && (
            <Button type="link" size="small" onClick={() => patch({ focusAreas: [], customFocus: "" })}>恢复智能规划</Button>
          )}
        </div>
        <div className="mi-focus-tags">
          {FOCUS_OPTIONS.map((focus) => (
            <Tag.CheckableTag
              key={focus}
              checked={value.focusAreas.includes(focus)}
              onChange={() => toggleFocus(focus)}
              className="mi-focus-tag"
            >
              {focus}
            </Tag.CheckableTag>
          ))}
        </div>
        <Input
          prefix={<FileTextOutlined />}
          value={value.customFocus}
          maxLength={120}
          placeholder="自定义侧重点，例如：重点考察Agent线上稳定性"
          onChange={(event) => patch({ customFocus: event.target.value })}
        />
      </section>
    </div>
  );
}
