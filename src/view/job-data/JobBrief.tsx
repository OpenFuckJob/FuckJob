import { useEffect, useState } from "react";
import { Button, Empty, Skeleton, Tag, Typography } from "antd";
import {
  EnvironmentOutlined,
  FileTextOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { fetchJobDescription } from "../../lib/job-description";
import type { JobDetail, ParsedJobDescription } from "../../types/job-detail";
import type { InterviewJobAnalysis } from "../../types/analysis";

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
}: {
  job: JobDetail;
  analysis?: InterviewJobAnalysis;
}) => {
  const [showRaw, setShowRaw] = useState(false);
  const [parsed, setParsed] = useState<ParsedJobDescription | null>(null);
  const platform = job.platform === "liepin" ? "猎聘" : "BOSS 直聘";

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
            <Tag color={job.platform === "liepin" ? "purple" : "green"}>{platform}</Tag>
            {job.is_send_resume ? <Tag color="blue">已投递</Tag> : <Tag>未投递</Tag>}
            {job.is_reply && <Tag color="cyan">已回复</Tag>}
            {analysis && !analysis.parse_error && (
              <Tag color={matchTone(analysis.match_score)}>
                匹配 {analysis.match_score} 分
              </Tag>
            )}
          </div>
        </div>
      </div>

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
