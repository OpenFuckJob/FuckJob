import { useState } from "react";
import {
  Card,
  Button,
  Checkbox,
  Space,
  Typography,
  Modal,
  Radio,
  Descriptions,
  Divider,
  Alert,
  message as antdMessage,
} from "antd";
import {
  ExportOutlined,
  ImportOutlined,
  DatabaseOutlined,
  FileZipOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commandErrorMessage, type CommandResult } from "@/types/command";
import type {
  ExportManifest,
  ImportResultStats,
  ImportStrategy,
} from "@/types/data-management";

const { Title, Text, Paragraph } = Typography;

export function DataManagementPanel() {
  const [includeConfig, setIncludeConfig] = useState(false);
  const [exporting, setExporting] = useState(false);

  const [inspecting, setInspecting] = useState(false);
  const [importing, setImporting] = useState(false);

  const [previewManifest, setPreviewManifest] = useState<ExportManifest | null>(
    null
  );
  const [selectedFilePath, setSelectedFilePath] = useState<string>("");
  const [importStrategy, setImportStrategy] = useState<ImportStrategy>("merge");
  const [importConfig, setImportConfig] = useState(false);
  const [modalVisible, setModalVisible] = useState(false);

  const handleExport = async () => {
    try {
      const now = new Date();
      const dateStr = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(
        2,
        "0"
      )}${String(now.getDate()).padStart(2, "0")}_${String(
        now.getHours()
      ).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}`;
      const defaultPath = `fuckjob_backup_${dateStr}.fjbackup`;

      const targetPath = await save({
        defaultPath,
        filters: [
          {
            name: "FuckJob 数据备份包 (*.fjbackup)",
            extensions: ["fjbackup", "zip"],
          },
        ],
      });

      if (!targetPath) return;

      setExporting(true);
      const res = await invoke<CommandResult<ExportManifest>>(
        "export_data_bundle",
        {
          path: targetPath,
          includeConfig,
        }
      );

      if (!res.success || !res.data) {
        throw new Error(commandErrorMessage(res.error, "导出数据失败"));
      }

      antdMessage.success(
        `数据打包导出成功！包含 ${res.data.stats.job_details_count} 条岗位数据。`
      );
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      antdMessage.error(`导出数据失败: ${msg}`);
    } finally {
      setExporting(false);
    }
  };

  const handleSelectImportFile = async () => {
    try {
      const filePath = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "FuckJob 数据备份包 (*.fjbackup, *.zip)",
            extensions: ["fjbackup", "zip"],
          },
        ],
      });

      if (!filePath || Array.isArray(filePath)) return;

      setInspecting(true);
      const res = await invoke<CommandResult<ExportManifest>>(
        "inspect_data_bundle",
        {
          path: filePath,
        }
      );

      if (!res.success || !res.data) {
        throw new Error(commandErrorMessage(res.error, "读取备份文件失败"));
      }

      setSelectedFilePath(filePath);
      setPreviewManifest(res.data);
      setImportConfig(res.data.includes_config);
      setModalVisible(true);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      antdMessage.error(`读取备份文件失败: ${msg}`);
    } finally {
      setInspecting(false);
    }
  };

  const handleConfirmImport = async () => {
    if (!selectedFilePath) return;

    try {
      setImporting(true);
      const res = await invoke<CommandResult<ImportResultStats>>(
        "import_data_bundle",
        {
          path: selectedFilePath,
          strategy: importStrategy,
          importConfig,
        }
      );

      if (!res.success || !res.data) {
        throw new Error(commandErrorMessage(res.error, "数据导入失败"));
      }

      const stats = res.data;
      const detailMsg =
        importStrategy === "merge"
          ? `新增岗位 ${stats.job_details_added} 条，更新岗位 ${stats.job_details_updated} 条，新增聊天记录 ${stats.chat_messages_added} 条。`
          : `已恢复岗位 ${stats.job_details_added} 条，聊天记录 ${stats.chat_messages_added} 条。`;

      antdMessage.success(`数据导入恢复成功！${detailMsg}`);
      setModalVisible(false);
      setPreviewManifest(null);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      antdMessage.error(`数据导入失败: ${msg}`);
    } finally {
      setImporting(false);
    }
  };

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <Card
        title={
          <Space>
            <DatabaseOutlined />
            <span>跨设备数据备份与共享</span>
          </Space>
        }
      >
        <Paragraph type="secondary">
          导出全量求职数据（包含已沟通岗位、沟通对话记录、AI 分析报告及简历模板）为归档文件（.fjbackup），方便在公司与家中等多台设备间共享合并求职进度或进行安全备份。
        </Paragraph>

        <Divider />

        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Title level={5}>数据导出</Title>
            <Space direction="vertical" style={{ marginBottom: 12 }}>
              <Checkbox
                checked={includeConfig}
                onChange={(e) => setIncludeConfig(e.target.checked)}
              >
                同时导出应用系统全局配置 (包含打招呼/自动回复模板与岗位筛选规则)
              </Checkbox>
            </Space>
            <div>
              <Button
                type="primary"
                icon={<ExportOutlined />}
                loading={exporting}
                onClick={handleExport}
              >
                导出数据归档包 (.fjbackup)
              </Button>
            </div>
          </div>

          <Divider />

          <div>
            <Title level={5}>数据导入与同步</Title>
            <Paragraph type="secondary">
              从备份包（.fjbackup）中导入恢复求职数据，支持智能合并去重或完全覆盖恢复。
            </Paragraph>
            <Button
              icon={<ImportOutlined />}
              loading={inspecting}
              onClick={handleSelectImportFile}
            >
              选择备份包文件并导入...
            </Button>
          </div>
        </Space>
      </Card>

      <Modal
        title={
          <Space>
            <FileZipOutlined />
            <span>导入备份数据确认</span>
          </Space>
        }
        open={modalVisible}
        onCancel={() => setModalVisible(false)}
        onOk={handleConfirmImport}
        confirmLoading={importing}
        okText="确认开始导入"
        cancelText="取消"
        width={560}
      >
        {previewManifest && (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Alert
              message="备份包解析成功"
              description={`导出时间：${previewManifest.exported_at} (由 FuckJob v${previewManifest.app_version} 生成)`}
              type="info"
              showIcon
            />

            <Descriptions title="备份数据概况" bordered size="small" column={2}>
              <Descriptions.Item label="已沟通岗位记录">
                {previewManifest.stats.job_details_count} 条
              </Descriptions.Item>
              <Descriptions.Item label="沟通聊天记录">
                {previewManifest.stats.chat_messages_count} 条
              </Descriptions.Item>
              <Descriptions.Item label="AI 岗位分析报告">
                {previewManifest.stats.interview_analyses_count} 条
              </Descriptions.Item>
              <Descriptions.Item label="简历快照与模板">
                {previewManifest.stats.user_resumes_count} 个
              </Descriptions.Item>
            </Descriptions>

            <Divider style={{ margin: "12px 0" }} />

            <div>
              <Text strong>选择导入策略：</Text>
              <Radio.Group
                style={{ width: "100%", marginTop: 8 }}
                value={importStrategy}
                onChange={(e) => setImportStrategy(e.target.value)}
              >
                <Space direction="vertical" style={{ width: "100%" }}>
                  <Radio value="merge">
                    <Text strong>智能合并 (推荐多设备同步使用)</Text>
                    <div style={{ color: "#666", paddingLeft: 24 }}>
                      按岗位 ID 去重。对于已存在的记录，保留修改时间最新的数据；不存在的记录自动追加。
                    </div>
                  </Radio>
                  <Radio value="overwrite">
                    <Text strong type="danger">
                      完全覆盖恢复
                    </Text>
                    <div style={{ color: "#666", paddingLeft: 24 }}>
                      用备份文件中的内容直接更新替换本地数据库，不会清除未冲突的数据。
                    </div>
                  </Radio>
                </Space>
              </Radio.Group>
            </div>

            {previewManifest.includes_config && (
              <div style={{ marginTop: 8 }}>
                <Checkbox
                  checked={importConfig}
                  onChange={(e) => setImportConfig(e.target.checked)}
                >
                  同步覆盖更新系统配置 (打招呼与筛选规则)
                </Checkbox>
              </div>
            )}
          </Space>
        )}
      </Modal>
    </Space>
  );
}
