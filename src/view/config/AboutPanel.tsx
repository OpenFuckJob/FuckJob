import { Button, Card, Divider, Space, Typography } from "antd";
import { InfoCircleOutlined, RocketOutlined, SyncOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { useUpdater } from "@/lib/updater";

const { Title, Text, Paragraph } = Typography;

export function AboutPanel() {
  const [version, setVersion] = useState("0.1.13");
  const { checking, checkForUpdates } = useUpdater();

  useEffect(() => {
    if (isTauri()) {
      void getVersion()
        .then((v) => setVersion(v))
        .catch((err) => console.warn("获取应用版本失败", err));
    }
  }, []);

  return (
    <Space direction="vertical" size="large" className="w-full">
      <div>
        <Title level={4} className="text-slate-900! m-0! flex items-center gap-2">
          <InfoCircleOutlined className="text-sky-500" />
          软件与更新
        </Title>
        <Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
          查看应用版本及系统更新状态
        </Text>
      </div>

      <Divider className="!my-0 opacity-10" />

      <Card className="rounded-2xl border border-slate-200/80 bg-white/85 shadow-xs">
        <div className="flex flex-col gap-6 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center gap-4">
            <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-sky-500 text-white shadow-md shadow-sky-500/20">
              <RocketOutlined className="text-2xl" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <Text className="text-lg font-bold text-slate-900">OfferFlow</Text>
                <Text className="text-xs text-slate-500 font-medium">智聘助手</Text>
                <span className="rounded-md bg-sky-100 px-2 py-0.5 text-xs font-semibold text-sky-700">
                  v{version}
                </span>
              </div>
              <Paragraph className="m-0! mt-1 text-xs text-slate-500">
                本地优先的求职自动化与智能辅助工具
              </Paragraph>
            </div>
          </div>

          <Button
            type="primary"
            icon={<SyncOutlined spin={checking} />}
            loading={checking}
            onClick={() => void checkForUpdates(true)}
            className="rounded-xl! h-10! font-bold bg-sky-600! hover:bg-sky-500!"
          >
            检查更新
          </Button>
        </div>
      </Card>

      <Card className="rounded-2xl border border-slate-200/80 bg-white/85 shadow-xs">
        <Title level={5} className="text-slate-900! mt-0! mb-3">
          关于 OfferFlow
        </Title>
        <Paragraph className="text-slate-600 text-xs leading-relaxed">
          OfferFlow 是一款注重隐私安全与效率提升的本地自动化求职工具，基于 Tauri 与 Web 技术构建。所有配置数据及生成的对话记录全量存储在本地。
        </Paragraph>
        <div className="flex flex-wrap gap-4 text-xs text-slate-500 border-t border-slate-100 pt-3 mt-4">
          <div>
            开源协议：<Text className="font-semibold text-slate-700">Apache-2.0</Text>
          </div>
          <div>
            开源仓库：
            <a
              href="https://github.com/OpenFuckJob/FuckJob"
              target="_blank"
              rel="noreferrer"
              className="text-sky-600 hover:underline font-medium"
            >
              OpenFuckJob/FuckJob
            </a>
          </div>
        </div>
      </Card>
    </Space>
  );
}
