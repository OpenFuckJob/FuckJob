import type { ReactNode } from "react";
import { Alert, Button, Space } from "antd";

export function AiFeatureGate({
  active,
  configured = active,
  onConfigure,
  children,
}: {
  active: boolean;
  configured?: boolean;
  onConfigure: () => void;
  children: ReactNode;
}) {
  if (active) return <>{children}</>;
  return (
    <Alert
      type={configured ? "warning" : "info"}
      showIcon
      message={configured ? "大模型已停用" : "AI 是可选功能，当前尚未配置大模型"}
      description={
        <Space direction="vertical">
          <Button size="small" type="primary" onClick={onConfigure}>
            {configured ? "去启用" : "配置大模型"}
          </Button>
        </Space>
      }
    />
  );
}
