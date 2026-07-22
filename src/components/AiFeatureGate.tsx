import type { ReactNode } from "react";
import { Alert, Button, Space } from "antd";

export function AiFeatureGate({ configured, onConfigure, children }: { configured: boolean; onConfigure: () => void; children: ReactNode }) {
  if (configured) return <>{children}</>;
  return (
    <Alert
      type="info"
      showIcon
      message="AI 是可选功能，当前尚未配置大模型"
      description={<Space direction="vertical"><span>其他本地功能仍可正常使用。</span><Button size="small" type="primary" onClick={onConfigure}>配置大模型</Button></Space>}
    />
  );
}
