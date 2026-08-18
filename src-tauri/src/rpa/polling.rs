//! 轮询回复的节奏计算。
//!
//! 全是纯函数，时间一律由调用方传入。这里算错不会崩，只会让机器人特征
//! 暴露出来——凌晨发消息、每次间隔精确到秒地一致、秒回——而这些症状在
//! 集成测试里根本看不出来，只能靠单测把边界钉死。

use crate::config::ReplyPollingConfig;

/// 一轮结束后该做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextWake {
    /// 睡这么多秒后跑下一轮
    Sleep(u64),
    /// 当前不在活跃时段，睡这么多秒后再判断一次
    OutsideActiveHours(u64),
}

impl NextWake {
    pub fn seconds(self) -> u64 {
        match self {
            Self::Sleep(seconds) | Self::OutsideActiveHours(seconds) => seconds,
        }
    }
}

/// 不在活跃时段时的重探间隔（秒）。
///
/// 不直接睡到时段开始：任务期间用户随时可能改配置，睡死过去的话改了也不生效。
const OUT_OF_HOURS_RECHECK_SECONDS: u64 = 60;

/// 判断某个小时是否落在活跃时段内。
///
/// 起点等于终点视为全天活跃，而不是「一秒都不活跃」——用户把两头调成一样时
/// 想表达的显然是不限制，让轮询彻底停摆只会被当成故障。
pub fn is_active_hour(config: &ReplyPollingConfig, hour: u32) -> bool {
    if !config.active_hours_enabled {
        return true;
    }
    let start = config.active_start_hour.min(23);
    let end = config.active_end_hour.min(24);
    if start == end {
        return true;
    }
    if start < end {
        // 常规时段，例如 9:00-22:00
        hour >= start && hour < end
    } else {
        // 跨零点时段，例如 22:00-06:00
        hour >= start || hour < end
    }
}

/// 算出本轮结束后要睡多久。
///
/// `jitter_source` 是一个 0..=1 的随机数，由调用方提供——把随机性留在外面，
/// 这个函数才可测。
pub fn next_wake(config: &ReplyPollingConfig, hour: u32, jitter_source: f64) -> NextWake {
    if !is_active_hour(config, hour) {
        return NextWake::OutsideActiveHours(OUT_OF_HOURS_RECHECK_SECONDS);
    }
    let base = config.interval_minutes.max(1).saturating_mul(60);
    let jitter = scale(config.jitter_seconds, jitter_source);
    NextWake::Sleep(base.saturating_add(jitter))
}

/// 回复前还需要额外等待的秒数。
///
/// 目标是「对方发出消息到我方回复」的间隔落在配置区间内。轮询间隔本身通常
/// 已经填满了这段目标，此时返回 0——再叠一次固定延迟只会拖垮轮询节奏，
/// 未读一多，一轮的耗时就会超过轮询间隔本身。
///
/// `elapsed_ms` 是对方最后一条消息距今的毫秒数。取不到时间戳时传 None，
/// 按「刚刚收到」处理，宁可多等也不要秒回。
pub fn humanize_delay_seconds(
    config: &ReplyPollingConfig,
    elapsed_ms: Option<i64>,
    delay_source: f64,
) -> u64 {
    let min = config.humanize_delay_min_seconds;
    let max = config.humanize_delay_max_seconds.max(min);
    let target = min.saturating_add(scale(max - min, delay_source));

    let elapsed_seconds = match elapsed_ms {
        // 负数意味着对方消息的时间戳在未来，多半是两端时钟不一致。
        // 当成「刚刚收到」而不是「已经等了很久」，方向上更安全
        Some(elapsed) => (elapsed.max(0) / 1000) as u64,
        None => 0,
    };
    target.saturating_sub(elapsed_seconds)
}

/// 读当前时钟与随机源，算出本轮结束后要睡多久。
///
/// 调用方用这个，纯函数版留给测试。随机数只在这一处取，
/// 免得每个调用点各自造一套抖动、抖出各不相同的分布
pub fn next_wake_now(config: &ReplyPollingConfig) -> NextWake {
    next_wake(config, current_hour(), random_unit())
}

/// 读当前随机源，算出回复前还需要额外等待的秒数。
pub fn humanize_delay_seconds_now(config: &ReplyPollingConfig, elapsed_ms: Option<i64>) -> u64 {
    humanize_delay_seconds(config, elapsed_ms, random_unit())
}

/// 当前是否落在活跃时段内。
pub fn is_active_now(config: &ReplyPollingConfig) -> bool {
    is_active_hour(config, current_hour())
}

fn current_hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

fn random_unit() -> f64 {
    use rand::Rng;
    rand::thread_rng().gen_range(0.0..=1.0)
}

/// 把 0..=1 的随机源映射到 0..=span。越界的源值一律夹回区间内
fn scale(span: u64, source: f64) -> u64 {
    if span == 0 {
        return 0;
    }
    let clamped = source.clamp(0.0, 1.0);
    (span as f64 * clamped).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ReplyPollingConfig {
        ReplyPollingConfig::default()
    }

    #[test]
    fn default_active_window_covers_daytime_only() {
        let config = config();
        assert!(is_active_hour(&config, 9));
        assert!(is_active_hour(&config, 21));
        assert!(!is_active_hour(&config, 22));
        assert!(!is_active_hour(&config, 3));
    }

    #[test]
    fn disabling_active_hours_makes_every_hour_active() {
        let mut config = config();
        config.active_hours_enabled = false;
        assert!(is_active_hour(&config, 3));
    }

    /// 起止相同表达的是「不限制」。当成空区间会让轮询彻底停摆，
    /// 用户看到的是任务在跑却什么都不做，比任何报错都难排查
    #[test]
    fn identical_bounds_mean_all_day_rather_than_never() {
        let mut config = config();
        config.active_start_hour = 9;
        config.active_end_hour = 9;
        assert!(is_active_hour(&config, 3));
        assert!(is_active_hour(&config, 15));
    }

    #[test]
    fn window_crossing_midnight_wraps_correctly() {
        let mut config = config();
        config.active_start_hour = 22;
        config.active_end_hour = 6;
        assert!(is_active_hour(&config, 23));
        assert!(is_active_hour(&config, 2));
        assert!(!is_active_hour(&config, 12));
    }

    #[test]
    fn jitter_stays_within_the_configured_span() {
        let config = config();
        let base = config.interval_minutes * 60;
        assert_eq!(next_wake(&config, 10, 0.0), NextWake::Sleep(base));
        assert_eq!(
            next_wake(&config, 10, 1.0),
            NextWake::Sleep(base + config.jitter_seconds)
        );
    }

    /// 越界的随机源不该把等待时间放大到离谱的量级
    #[test]
    fn out_of_range_jitter_sources_are_clamped() {
        let config = config();
        let base = config.interval_minutes * 60;
        assert_eq!(next_wake(&config, 10, -5.0), NextWake::Sleep(base));
        assert_eq!(
            next_wake(&config, 10, 99.0),
            NextWake::Sleep(base + config.jitter_seconds)
        );
    }

    #[test]
    fn outside_active_hours_rechecks_instead_of_sleeping_until_dawn() {
        let config = config();
        assert_eq!(
            next_wake(&config, 3, 0.5),
            NextWake::OutsideActiveHours(OUT_OF_HOURS_RECHECK_SECONDS)
        );
    }

    /// 间隔配成 0 会让轮询变成忙循环，兜到 1 分钟
    #[test]
    fn zero_interval_falls_back_to_one_minute() {
        let mut config = config();
        config.interval_minutes = 0;
        config.jitter_seconds = 0;
        assert_eq!(next_wake(&config, 10, 0.0), NextWake::Sleep(60));
    }

    /// 轮询间隔本身就提供了绝大部分「人味」，不该在它之上再叠一次固定延迟
    #[test]
    fn elapsed_time_since_the_peer_message_counts_toward_the_delay() {
        let config = config();
        assert_eq!(humanize_delay_seconds(&config, Some(300_000), 0.0), 0);
        assert_eq!(humanize_delay_seconds(&config, Some(10_000), 0.0), 20);
    }

    #[test]
    fn a_just_arrived_message_waits_the_full_target() {
        let config = config();
        assert_eq!(humanize_delay_seconds(&config, Some(0), 0.0), 30);
        assert_eq!(humanize_delay_seconds(&config, Some(0), 1.0), 120);
    }

    /// 时间戳缺失或来自未来时按「刚刚收到」处理，宁可多等也不要秒回
    #[test]
    fn missing_or_future_timestamps_are_treated_as_just_arrived() {
        let config = config();
        assert_eq!(humanize_delay_seconds(&config, None, 0.0), 30);
        assert_eq!(humanize_delay_seconds(&config, Some(-90_000), 0.0), 30);
    }

    /// 区间配反了不该 panic，按下界收敛成固定延迟
    #[test]
    fn an_inverted_delay_range_collapses_to_its_lower_bound() {
        let mut config = config();
        config.humanize_delay_min_seconds = 90;
        config.humanize_delay_max_seconds = 10;
        assert_eq!(humanize_delay_seconds(&config, Some(0), 1.0), 90);
    }
}
