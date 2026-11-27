import { useEffect, useState } from "react";
import { Button, Empty, Input, Popconfirm, Select, Skeleton, Space, Tag, Typography, message } from "antd";
import { invoke } from "@tauri-apps/api/core";
import { commandErrorMessage, type CommandResult } from "../../types/command";
import {
  CalendarOutlined,
  EditOutlined,
  EnvironmentOutlined,
  FileTextOutlined,
  SaveOutlined,
  StopOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { fetchJobDescription } from "../../lib/job-description";
import type { JobDetail, JobPipelineStage, ParsedJobDescription } from "../../types/job-detail";
import type { InterviewJobAnalysis } from "../../types/analysis";

const PIPELINE_STAGE_OPTIONS: Array<{ value: JobPipelineStage; label: string }> = [
  { value: "pending", label: "已打招呼" },
  { value: "applied", label: "已发简历" },
  { value: "replied", label: "HR 已回复" },
  { value: "interview_scheduled", label: "已约面" },
  { value: "interviewing", label: "面试中" },
  { value: "offer", label: "已拿 Offer" },
  { value: "rejected", label: "已婉拒" },
  { value: "archived", label: "已归档" },
];

/** 小节的语义配色：职责说「做什么」，要求说「要什么」，加分项是锦上添花 */
const sectionTone = (title: string): "duty" | "require" | "bonus" | "plain" => {
  if (/职责|描述|详情|内容/.test(title)) return "duty";
  if (/要求|资格|能力/.test(title)) return "require";
  if (/加分|福利|待遇|我们提供/.test(title)) return "bonus";
  return "plain";
};

const matchTone = (score: number): string =>
  score >= 80 ? "green" : score >= 60 ? "gold" : "red";

/** 岗位表格里的时间是 `2026-08-06 16:08:27`，详情里只到分钟就够 */
const shortTime = (value?: string | null): string =>
  value ? value.slice(0, 16) : "-";

function MetaItem({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="brief-meta-item">
      <span className="brief-meta-label">{label}</span>
      <span className="brief-meta-value">{value}</span>
    </div>
  );
}

/**
 * 岗位原始信息的可视化。
 *
 * 抓到的 JD 原文噪声很重，清洗与结构化统一在 Rust 侧的 `job_description`
 * 里完成——和喂给模型的是同一份文本，前端不再自己洗一遍。这里只负责把
 * 小节铺开，并保证不出现「明明抓到了数据，页面上什么都没有」。
 */
const JobBrief = ({
  job,
  analysis,
  onDeliveryResolved,
  onJobUpdated,
  onBlacklistCompany,
}: {
  job: JobDetail;
  analysis?: InterviewJobAnalysis;
  onDeliveryResolved?: (updated: JobDetail) => void;
  onJobUpdated?: (updated: JobDetail) => void;
  onBlacklistCompany?: (companyName: string) => void;
}) => {
  const [showRaw, setShowRaw] = useState(false);
  const [parsed, setParsed] = useState<ParsedJobDescription | null>(null);
  const [stageDraft, setStageDraft] = useState<JobPipelineStage | undefined>(
    job.pipeline_stage ?? undefined,
  );
  const [interviewAtDraft, setInterviewAtDraft] = useState(job.interview_at ?? "");
  const [notesDraft, setNotesDraft] = useState(job.notes ?? "");
  const [savingCrm, setSavingCrm] = useState(false);

  useEffect(() => {
    setStageDraft(job.pipeline_stage ?? undefined);
    setInterviewAtDraft(job.interview_at ?? "");
    setNotesDraft(job.notes ?? "");
  }, [job.id, job.interview_at, job.notes, job.pipeline_stage]);

  const platform = job.platform === "51job" ? "前程无忧" : job.platform === "liepin" ? "猎聘" : "BOSS 直聘";
  const platformTagColor = job.platform === "51job" ? "orange" : job.platform === "liepin" ? "purple" : "green";
  const crmDirty =
    (stageDraft ?? null) !== (job.pipeline_stage ?? null) ||
    interviewAtDraft.trim() !== (job.interview_at ?? "").trim() ||
    notesDraft.trim() !== (job.notes ?? "").trim();

  const saveCrm = async () => {
    setSavingCrm(true);
    try {
      const updated: JobDetail = {
        ...job,
        pipeline_stage: stageDraft ?? null,
        interview_at: interviewAtDraft.trim() || null,
        notes: notesDraft.trim() || null,
      };
      const result = await invoke<CommandResult<void>>("job_update", {
        id: job.id,
        job: updated,
      });
      if (!result.success) {
        message.error(commandErrorMessage(result.error, "保存面试进度失败"));
        return;
      }
      message.success("进度与笔记已保存");
      onJobUpdated?.(updated);
      onDeliveryResolved?.(updated);
    } catch (error) {
      message.error(String(error));
    } finally {
      setSavingCrm(false);
    }
  };
  const resolveDelivery = async (delivered: boolean) => {
    try {
      const result = await invoke<CommandResult<JobDetail>>("job51_resolve_delivery", { id: job.id, delivered });
      if (!result.success || !result.data) { message.error(commandErrorMessage(result.error, "更新投递状态失败")); return; }
      onDeliveryResolved?.(result.data);
    } catch (error) { message.error(String(error)); }
  };

  useEffect(() => {
    let stale = false;
    setParsed(null);
    void fetchJobDescription(job.id).then((result) => {
      if (!stale) setParsed(result);
    });
    return () => {
      stale = true;
    };
  }, [job.id]);

  return (
    <div className="job-brief">
      {/* ── 头部：一眼看清是什么岗位、什么状态 ── */}
      <div className="brief-head">
        <div className="brief-head-main">
          <Typography.Title level={4} className="brief-title">
            {job.title}
          </Typography.Title>
          <div className="brief-subtitle">
            <span className="brief-company">{job.company_name || "未知公司"}</span>
            {job.location && <span className="brief-dot">·</span>}
            {job.location && <span>{job.location.trim()}</span>}
          </div>
        </div>
        <div className="brief-head-side">
          {job.salary && <div className="brief-salary">{job.salary}</div>}
          <div className="brief-badges">
            <Tag color={platformTagColor}>{platform}</Tag>
            {job.resume_delivery_pending ? <Tag color="orange">投递待确认</Tag> : job.is_send_resume ? <Tag color="blue">已投递</Tag> : <Tag>未投递</Tag>}
            {job.is_reply && <Tag color="cyan">已回复</Tag>}
            {analysis && !analysis.parse_error && (
              <Tag color={matchTone(analysis.match_score)}>
                匹配 {analysis.match_score} 分
              </Tag>
            )}
            {onBlacklistCompany && job.company_name.trim() && (
              <Popconfirm
                title={`将「${job.company_name}」加入公司黑名单？`}
                description="加入后，当前求职方案在后续投递中会自动跳过该公司的全部岗位。"
                okText="确认拉黑"
                cancelText="取消"
                onConfirm={() => {
                  onBlacklistCompany(job.company_name);
                  message.success(`已将「${job.company_name}」加入公司排除词`);
                }}
              >
                <Button size="small" type="text" danger icon={<StopOutlined />}>
                  拉黑公司
                </Button>
              </Popconfirm>
            )}
          </div>
        </div>
      </div>

      {/* ── 面试进度与笔记（个人求职 CRM） ── */}
      <div className="brief-crm">
        <div className="brief-crm-row">
          <div className="brief-crm-field">
            <span className="brief-meta-label">推进阶段</span>
            <Select<JobPipelineStage>
              size="small"
              allowClear
              placeholder="自动判定"
              style={{ width: 136 }}
              value={stageDraft}
              options={PIPELINE_STAGE_OPTIONS}
              onChange={(val) => setStageDraft(val)}
            />
          </div>
          <div className="brief-crm-field" style={{ flex: 1, minWidth: 180 }}>
            <span className="brief-meta-label">
              <CalendarOutlined /> 面试时间
            </span>
            <Input
              size="small"
              allowClear
              placeholder="例如：本周四 14:30 一面（腾讯会议）"
              value={interviewAtDraft}
              onChange={(event) => setInterviewAtDraft(event.target.value)}
            />
          </div>
          <Button
            size="small"
            type="primary"
            icon={<SaveOutlined />}
            loading={savingCrm}
            disabled={!crmDirty}
            onClick={() => void saveCrm()}
          >
            保存进度
          </Button>
        </div>
        <div className="brief-crm-notes">
          <Input.TextArea
            autoSize={{ minRows: 2, maxRows: 5 }}
            placeholder="记录一面/二面复盘、面试官关注点、薪资期望或薪资包对比..."
            value={notesDraft}
            onChange={(event) => setNotesDraft(event.target.value)}
          />
          {!crmDirty && job.notes && (
            <Typography.Text type="secondary" style={{ fontSize: 11, marginTop: 4, display: "inline-flex", alignItems: "center", gap: 4 }}>
              <EditOutlined /> 已保存到本地
            </Typography.Text>
          )}
        </div>
      </div>

      {job.platform === "51job" && job.resume_delivery_pending && <Space wrap style={{ marginBottom: 16 }}>
        <Typography.Text type="secondary">此前提交结果未确认，自动重投已暂停。</Typography.Text>
        <Popconfirm title="已在51job核实投递成功？" onConfirm={() => void resolveDelivery(true)}><Button size="small">确认已投递</Button></Popconfirm>
        <Popconfirm title="允许下次任务重新投递？" description="此前提交结果未确认，重新投递可能重复申请。" onConfirm={() => void resolveDelivery(false)}><Button size="small">允许重试</Button></Popconfirm>
      </Space>}

      {/* ── 时间线与来源，都是表格里看不全的字段 ── */}
      <div className="brief-meta">
        <MetaItem label="收录时间" value={shortTime(job.created_at)} />
        <MetaItem label="投递时间" value={shortTime(job.resume_sent_at)} />
        <MetaItem label="最近更新" value={shortTime(job.updated_at)} />
        {parsed && parsed.highlights.length > 0 && (
          <MetaItem
            label="岗位条件"
            value={
              <span className="brief-chips">
                {parsed.highlights.map((item) => (
                  <span className="brief-chip" key={item}>
                    {item}
                  </span>
                ))}
              </span>
            }
          />
        )}
      </div>

      {/* ── JD 正文。岗位头部本地就有，只有正文要等后端解析 ── */}
      {!parsed ? (
        <Skeleton active paragraph={{ rows: 6 }} />
      ) : parsed.empty ? (
        <Empty
          className="brief-empty"
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="这个岗位还没抓到岗位描述"
        />
      ) : parsed.sections.length > 0 ? (
        <div className="brief-sections">
          {parsed.sections.map((section, index) => (
            <section
              className={`brief-section is-${sectionTone(section.title)}`}
              key={`${section.title}-${index}`}
            >
              <h4 className="brief-section-title">
                <FileTextOutlined />
                {section.title || "岗位描述"}
              </h4>
              <ol className="brief-section-list">
                {section.items.map((item, itemIndex) => (
                  <li key={itemIndex}>{item}</li>
                ))}
              </ol>
            </section>
          ))}
        </div>
      ) : (
        // 猎聘存的是列表卡片文本，本就没有 JD 正文。有效字段已经提到上面的
        // 「岗位条件」里，原文留给下面的折叠区，避免同样的内容铺两遍
        <Empty
          className="brief-empty"
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="该平台的列表页没有岗位描述，可展开下方原文查看抓取到的卡片信息"
        />
      )}

      {/* ── 附加信息 ── */}
      {parsed && (parsed.workplace || parsed.recruiter) && (
        <div className="brief-extra">
          {parsed.workplace && (
            <div className="brief-extra-item">
              <EnvironmentOutlined />
              <span>{parsed.workplace}</span>
            </div>
          )}
          {parsed.recruiter && (
            <div className="brief-extra-item">
              <UserOutlined />
              <span>
                {parsed.recruiter.name}
                {parsed.recruiter.role && ` · ${parsed.recruiter.role}`}
                {parsed.recruiter.company &&
                  parsed.recruiter.company !== job.company_name &&
                  ` · ${parsed.recruiter.company}`}
              </span>
              {parsed.recruiter.status && (
                <Tag className="brief-status">{parsed.recruiter.status}</Tag>
              )}
            </div>
          )}
        </div>
      )}

      {/* 解析规则跟着平台页面结构走，页面改版时留个看原文的出口 */}
      {parsed && !parsed.empty && (
        <div className="brief-raw">
          <Button type="link" size="small" onClick={() => setShowRaw((on) => !on)}>
            {showRaw ? "收起原文" : "查看抓取原文"}
          </Button>
          {showRaw && <pre className="brief-raw-text">{parsed.clean_text}</pre>}
        </div>
      )}
    </div>
  );
};

export default JobBrief;
