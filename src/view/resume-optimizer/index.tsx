import { useCallback, useState } from "react";
import { Alert, Button, Card, Typography } from "antd";
import { RobotOutlined, CopyOutlined, ThunderboltOutlined } from "@ant-design/icons";
import type { AppRuntimeConfig } from "@/types/app-config";
import { MockInterviewPanel } from "./MockInterviewPanel";

/* ────────── helpers kept for ConfigPage use ────────── */

export interface ResumeMarkdownSection {
  title: string;
  start: number;
  end: number;
}

export function extractSections(content: string): ResumeMarkdownSection[] {
  const lines = content.split("\n");
  const sections: ResumeMarkdownSection[] = [];

  const firstHeaderIdx = lines.findIndex((line) => line.match(/^##\s+/));
  if (firstHeaderIdx > 0) {
    sections.push({ title: "个人信息", start: 0, end: firstHeaderIdx });
  } else if (firstHeaderIdx === -1 && lines.some((l) => l.trim())) {
    sections.push({ title: "个人信息", start: 0, end: lines.length });
  }

  let currentSection: { title: string; start: number } | null = null;
  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(/^##\s+(.+)/);
    if (match) {
      if (currentSection) {
        sections.push({ ...currentSection, end: i });
      }
      currentSection = { title: match[1].trim(), start: i };
    }
  }
  if (currentSection) {
    sections.push({ ...currentSection, end: lines.length });
  }

  return sections;
}

export function replaceSectionContent(
  content: string,
  section: ResumeMarkdownSection,
  nextSectionContent: string,
): string {
  const lines = content.split("\n");
  const before = lines.slice(0, section.start).join("\n");
  const after = lines.slice(section.end).join("\n");
  return [before, nextSectionContent, after].filter(Boolean).join("\n");
}

export function findSectionIndexByRenderedTitle(
  sections: ResumeMarkdownSection[],
  renderedTitle: string,
): number {
  const normalizedTitle = renderedTitle.replace(/^#+\s*/, "").trim();
  return sections.findIndex(
    (section) => section.title.trim() === normalizedTitle,
  );
}

/* ────────── Standalone mock interview page ────────── */

export interface ResumeOptimizerPageProps {
  config: AppRuntimeConfig;
  onOpenLlmConfig: () => void;
}

function ResumeOptimizerPage({ config, onOpenLlmConfig }: ResumeOptimizerPageProps) {
  const [interviewActive, setInterviewActive] = useState(false);
  const resumeContent = (config.resume_config.resume_content ?? "").trim();

  const handleCopyResume = useCallback(async () => {
    if (!resumeContent) return;
    try {
      await navigator.clipboard.writeText(resumeContent);
    } catch { /* ignore */ }
  }, [resumeContent]);

  // Entry guard checks
  const canStart = !!config.llm_config && !!resumeContent;

  // When interview is active, render the full-page chat panel
  if (interviewActive) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          height: "100%",
          maxWidth: 860,
          margin: "0 auto",
          width: "100%",
        }}
      >
        <MockInterviewPanel
          resumeContent={resumeContent}
          onBack={() => setInterviewActive(false)}
          onApply={async () => {
            // Apply is handled in ConfigPage; here we just dismiss the panel.
            setInterviewActive(false);
          }}
        />
      </div>
    );
  }

  // Entry screen
  return (
    <div style={{ maxWidth: 720, margin: "0 auto" }}>
      <Typography.Title level={4} style={{ marginBottom: 16 }}>
        <RobotOutlined style={{ marginRight: 10 }} />
        AI 模拟面试
      </Typography.Title>
      <Typography.Paragraph type="secondary">
        基于简历内容，AI 面试官通过 5 轮追问挖掘你的项目事实（技术深度、个人贡献、量化结果、问题处理、表达可信度），
        最终生成简历优化建议章节，可一键采纳到简历配置。
      </Typography.Paragraph>

      <Card
        style={{ marginTop: 20 }}
        styles={{ body: { padding: 24, textAlign: "center" } }}
      >
        {!config.llm_config ? (
          <Alert
            type="warning"
            showIcon
            message="未配置 AI 模型"
            description="模拟面试需要大模型支持，请先在配置中心完成模型配置。"
            action={
              <Button type="primary" size="small" onClick={onOpenLlmConfig}>
                去配置
              </Button>
            }
          />
        ) : !resumeContent ? (
          <Alert
            type="info"
            showIcon
            message="简历内容为空"
            description={
              <span>
                模拟面试需要简历内容作为对话基础。请在
                <Button type="link" size="small" onClick={onOpenLlmConfig} style={{ padding: "0 4px" }}>
                  配置中心 → 简历配置
                </Button>
                中填写或粘贴你的 Markdown 简历。
              </span>
            }
          />
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 16, alignItems: "center" }}>
            <div
              style={{
                padding: "16px 24px",
                background: "linear-gradient(135deg, #f0f5ff 0%, #e6f0ff 100%)",
                borderRadius: 12,
                border: "1px solid rgba(22,119,255,0.15)",
                width: "100%",
                boxSizing: "border-box",
              }}
            >
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                当前简历已就绪（{resumeContent.length} 字）
              </Typography.Text>
              <div style={{ display: "flex", justifyContent: "center", gap: 8, marginTop: 8 }}>
                <Button
                  type="primary"
                  size="large"
                  icon={<ThunderboltOutlined />}
                  disabled={!canStart}
                  onClick={() => setInterviewActive(true)}
                >
                  开始模拟面试
                </Button>
                <Button size="large" icon={<CopyOutlined />} onClick={() => void handleCopyResume()}>
                  复制简历
                </Button>
              </div>
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}

export default ResumeOptimizerPage;
