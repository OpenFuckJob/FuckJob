import { useCallback, useEffect, useState } from "react";
import { Button, Drawer, Empty, List, Modal, Space, Tag, Tooltip, Typography, message } from "antd";
import { ExclamationCircleOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type { ManualReviewRecord } from "../../types/manual-review";
import {
  MANUAL_REVIEW_REASON_COLORS,
  MANUAL_REVIEW_REASON_LABELS,
  formatRelativeTime,
} from "../../types/manual-review";

const REFRESH_INTERVAL_MS = 30_000;

/**
 * 待人工处理的会话数据源。
 *
 * 单独抽成 hook 是因为它有两个消费方：工作台顶部的统计磁贴只要一个数字，
 * 抽屉才需要完整列表。让磁贴去挂一个隐藏的列表组件来拿计数，
 * 会变成两处各轮询一次。
 */
export function useManualReview() {
  const [records, setRecords] = useState<ManualReviewRecord[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<ManualReviewRecord[]>>("manual_review_list");
      if (result.success && result.data) {
        setRecords(result.data);
      }
    } catch (error) {
      // 列表拉取失败不打扰用户：下一次轮询多半就好了，弹窗反而更烦
      console.error("拉取待人工处理列表失败", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  const resolve = useCallback(async (record: ManualReviewRecord) => {
    try {
      const result = await invoke<CommandResult<null>>("manual_review_resolve", {
        platform: record.platform,
        conversationId: record.conversation_id,
      });
      if (!result.success) {
        message.error(commandErrorMessage(result.error, "标记失败"));
        return;
      }
      setRecords((current) => current.filter((item) => item.id !== record.id));
    } catch (error) {
      message.error(`标记失败：${error}`);
    }
  }, []);

  const clear = useCallback(async () => {
    const result = await invoke<CommandResult<null>>("manual_review_clear");
    if (!result.success) {
      message.error(commandErrorMessage(result.error, "清空失败"));
      return;
    }
    setRecords([]);
  }, []);

  return { records, loading, refresh, resolve, clear };
}

interface Props {
  open: boolean;
  onClose: () => void;
  records: ManualReviewRecord[];
  loading: boolean;
  onResolve: (record: ManualReviewRecord) => void;
  onClear: () => void;
  /** 跳到该岗位的会话记录。岗位归属缺失的条目跳不了，入口会隐藏 */
  onOpenConversation?: (jobId: string) => void;
}

/**
 * 待人工处理的会话。
 *
 * 存在的理由：点开会话就会让它变成已读，但 AI 未必会回。轮询模式下用户不会
 * 盯着日志，那些「读了却没回」的消息会静默消失，手机上连未读红点都没有。
 * 这里是它们唯一的出口。
 */
export default function ManualReviewDrawer({
  open,
  onClose,
  records,
  loading,
  onResolve,
  onClear,
  onOpenConversation,
}: Props) {
  const confirmClear = useCallback(() => {
    Modal.confirm({
      title: "清空待处理列表",
      icon: <ExclamationCircleOutlined />,
      content: `将移除全部 ${records.length} 条记录。这些会话本身不受影响，只是不再提醒你。`,
      okText: "清空",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: onClear,
    });
  }, [onClear, records.length]);

  return (
    <Drawer
      title="待你处理"
      width={480}
      open={open}
      onClose={onClose}
      extra={
        records.length > 0 ? (
          <Button size="small" type="text" danger onClick={confirmClear}>
            清空
          </Button>
        ) : null
      }
    >
      {records.length === 0 ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {loading
                ? "加载中"
                : "没有需要你接手的会话。AI 读了消息却决定不回时，会把会话放到这里，不会让它悄悄过去"}
            </Typography.Text>
          }
        />
      ) : (
        <List
          dataSource={records}
          renderItem={(record) => (
            <List.Item
              key={record.id}
              actions={[
                ...(record.job_id && onOpenConversation
                  ? [
                      <Button
                        key="open"
                        size="small"
                        type="link"
                        onClick={() => onOpenConversation(record.job_id)}
                      >
                        查看会话
                      </Button>,
                    ]
                  : []),
                <Button key="resolve" size="small" type="link" onClick={() => onResolve(record)}>
                  已处理
                </Button>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Space size={8} wrap>
                    <Tag
                      color={MANUAL_REVIEW_REASON_COLORS[record.reason]}
                      style={{ marginInlineEnd: 0 }}
                    >
                      {MANUAL_REVIEW_REASON_LABELS[record.reason]}
                    </Tag>
                    <Typography.Text strong style={{ fontSize: 13 }}>
                      {record.job_name || "未识别岗位"}
                    </Typography.Text>
                    {record.company_name ? (
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        {record.company_name}
                      </Typography.Text>
                    ) : null}
                    {record.hit_count > 1 ? (
                      <Tooltip title="该会话反复触发，对方可能一直在等回复">
                        <Tag style={{ marginInlineEnd: 0 }}>{record.hit_count} 次</Tag>
                      </Tooltip>
                    ) : null}
                  </Space>
                }
                description={
                  <Space direction="vertical" size={2} style={{ width: "100%" }}>
                    <Typography.Text style={{ fontSize: 12 }}>{record.detail}</Typography.Text>
                    {record.last_message ? (
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        对方：{record.last_message}
                      </Typography.Text>
                    ) : null}
                    <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                      {formatRelativeTime(record.updated_at)}
                    </Typography.Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      )}
    </Drawer>
  );
}
