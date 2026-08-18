import { Col, Form, Row, Select, Switch, Typography } from "antd";
import {
  DEFAULT_REPLY_POLLING_CONFIG,
  type ReplyPollingConfig,
} from "../../types/app-config";
import { NumberField } from "@/components/NumberField";

const { Text } = Typography;

const HOUR_OPTIONS = Array.from({ length: 24 }, (_, hour) => ({
  value: hour,
  label: `${hour}:00`,
}));

interface Props {
  config: ReplyPollingConfig;
  onChange: (next: Partial<ReplyPollingConfig>) => void;
}

/**
 * 自动回复的轮询节奏。
 *
 * 默认值整体偏保守，取舍点是「不被认成机器人」而不是「回得最快」：
 * 固定节律、秒回、凌晨活跃，这三样单拎出来都是明显的自动化特征。
 */
export default function ReplyPollingSection({ config, onChange }: Props) {
  return (
    <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-5">
      <div>
        <Text className="text-slate-900 font-bold block">轮询节奏</Text>
        <Text className="text-slate-500 text-xs">
          「轮询回复」任务和「周期投递」的等待期都按这套节奏检查新消息
        </Text>
      </div>

      <Row gutter={[24, 16]}>
        <Col xs={24} md={8}>
          <Form.Item label="检查间隔" extra="多久去看一次有没有新消息。" className="!mb-0">
            <NumberField
              min={1}
              precision={0}
              fallback={DEFAULT_REPLY_POLLING_CONFIG.interval_minutes}
              value={config.interval_minutes}
              style={{ width: "100%" }}
              addonAfter="分钟"
              onChange={(value) => onChange({ interval_minutes: value })}
            />
          </Form.Item>
        </Col>
        <Col xs={24} md={8}>
          <Form.Item
            label="随机浮动"
            extra="在间隔上叠加的随机量。分秒不差的固定节律本身就像机器。"
            className="!mb-0"
          >
            <NumberField
              min={0}
              precision={0}
              fallback={DEFAULT_REPLY_POLLING_CONFIG.jitter_seconds}
              value={config.jitter_seconds}
              style={{ width: "100%" }}
              addonAfter="秒内"
              onChange={(value) => onChange({ jitter_seconds: value })}
            />
          </Form.Item>
        </Col>
        <Col xs={24} md={8}>
          <Form.Item
            label="单轮最多处理"
            extra="未读堆积时超出的部分留到下一轮，避免一轮跑得比间隔还久。"
            className="!mb-0"
          >
            <NumberField
              min={1}
              precision={0}
              fallback={DEFAULT_REPLY_POLLING_CONFIG.max_conversations_per_round}
              value={config.max_conversations_per_round}
              style={{ width: "100%" }}
              addonAfter="个会话"
              onChange={(value) => onChange({ max_conversations_per_round: value })}
            />
          </Form.Item>
        </Col>
      </Row>

      <div className="space-y-4 pt-4 border-t border-slate-200/80">
        <div className="flex items-center justify-between">
          <div>
            <Text className="text-slate-900 font-bold block">只在指定时段回复</Text>
            <Text className="text-slate-500 text-xs">
              凌晨三点秒回 HR 是最明显的机器人特征。只管回复，投递不受影响
            </Text>
          </div>
          <Switch
            aria-label="只在指定时段回复"
            checked={config.active_hours_enabled}
            onChange={(checked) => onChange({ active_hours_enabled: checked })}
          />
        </div>

        {config.active_hours_enabled && (
          <Row gutter={[24, 16]}>
            <Col xs={12} md={8}>
              <Form.Item label="从" className="!mb-0">
                <Select
                  aria-label="回复时段起点"
                  style={{ width: "100%" }}
                  value={config.active_start_hour}
                  options={HOUR_OPTIONS}
                  onChange={(value) => onChange({ active_start_hour: value })}
                />
              </Form.Item>
            </Col>
            <Col xs={12} md={8}>
              <Form.Item
                label="到"
                extra={
                  config.active_start_hour === config.active_end_hour
                    ? "起止相同表示全天回复"
                    : undefined
                }
                className="!mb-0"
              >
                <Select
                  aria-label="回复时段终点"
                  style={{ width: "100%" }}
                  value={config.active_end_hour}
                  options={HOUR_OPTIONS}
                  onChange={(value) => onChange({ active_end_hour: value })}
                />
              </Form.Item>
            </Col>
          </Row>
        )}
      </div>

      <div className="space-y-4 pt-4 border-t border-slate-200/80">
        <div>
          <Text className="text-slate-900 font-bold block">回复前等待</Text>
          <Text className="text-slate-500 text-xs">
            指「对方发出消息」到「我方回复」的目标间隔。检查间隔本身通常已经把它填满，
            这时不会额外等待——只有刚好撞上对方刚发完才真的等
          </Text>
        </div>
        <Row gutter={[24, 16]}>
          <Col xs={12} md={8}>
            <Form.Item label="最少" className="!mb-0">
              <NumberField
                min={0}
                precision={0}
                fallback={DEFAULT_REPLY_POLLING_CONFIG.humanize_delay_min_seconds}
                value={config.humanize_delay_min_seconds}
                style={{ width: "100%" }}
                addonAfter="秒"
                onChange={(value) => onChange({ humanize_delay_min_seconds: value })}
              />
            </Form.Item>
          </Col>
          <Col xs={12} md={8}>
            <Form.Item
              label="最多"
              extra={
                config.humanize_delay_max_seconds < config.humanize_delay_min_seconds
                  ? "小于最少值时按最少值固定等待"
                  : undefined
              }
              className="!mb-0"
            >
              <NumberField
                min={0}
                precision={0}
                fallback={DEFAULT_REPLY_POLLING_CONFIG.humanize_delay_max_seconds}
                value={config.humanize_delay_max_seconds}
                style={{ width: "100%" }}
                addonAfter="秒"
                onChange={(value) => onChange({ humanize_delay_max_seconds: value })}
              />
            </Form.Item>
          </Col>
        </Row>
      </div>
    </div>
  );
}
