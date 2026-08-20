//! 周期投递的定时计划。
//!
//! 和 [`super::polling`] 一样全是纯函数，当前时刻一律由调用方传入。定时这件事
//! 一旦算错，症状是「到点没开投」或者「该收工了还在投」——两者都要等上几个小时
//! 才看得出来，集成测试根本覆盖不到，只能靠单测把边界钉死。

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

/// 一天里的分钟数上限。窗口用「零点起的分钟数」表示而不是小时，
/// 是为了支持 09:30 这种半点边界
pub const MINUTES_PER_DAY: u32 = 24 * 60;

/// 每日投递时段，左闭右开。
///
/// 起点等于终点视为全天投递，而不是「一分钟都不投」——用户把两头调成一样时
/// 想表达的显然是不限制，让投递彻底停摆只会被当成故障。这条语义与
/// [`super::polling::is_active_hour`] 保持一致
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyWindow {
    pub start_minute: u32,
    pub end_minute: u32,
}

impl DailyWindow {
    pub fn new(start_minute: u32, end_minute: u32) -> Self {
        Self {
            start_minute: start_minute.min(MINUTES_PER_DAY),
            end_minute: end_minute.min(MINUTES_PER_DAY),
        }
    }

    /// 起止相同 = 全天，此时窗口不构成任何约束
    fn is_all_day(&self) -> bool {
        self.start_minute == self.end_minute
    }
}

/// 一次周期投递任务的完整计划。
///
/// 全字段 `serde(default)`：前端只传 `interval_minutes` 时构造出来的计划
/// 等价于改造前的行为——无时段限制、不自动结束、单轮不设上界
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PeriodicPlan {
    /// 两轮投递之间的间隔（分钟）
    pub interval_minutes: u64,

    /// 每日投递时段，可以有多段（例如上午一段、下午一段）。空表示全天可投。
    ///
    /// 一律经 [`normalize_windows`] 规整后再存进来：重叠与首尾相接的段已合并，
    /// 跨零点的段已拆成两截，因此这里的每一段都满足 `start < end`
    #[serde(default)]
    pub windows: Vec<DailyWindow>,

    /// 整个任务的结束时刻。None 表示手动停止前一直跑
    #[serde(default)]
    pub run_until: Option<DateTime<Local>>,

    /// 单轮最多打招呼多少条，0 表示不限
    #[serde(default)]
    pub max_greets_per_round: u32,

    /// 单轮最长跑多少分钟，0 表示不限
    #[serde(default)]
    pub max_round_minutes: u64,
}

impl PeriodicPlan {
    /// 只有间隔、其余全不限的计划。改造前的周期投递就是这个形态
    pub fn every(interval_minutes: u64) -> Self {
        Self {
            interval_minutes,
            windows: Vec::new(),
            run_until: None,
            max_greets_per_round: 0,
            max_round_minutes: 0,
        }
    }
}

/// 某一时刻周期任务该处于的状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodicState {
    /// 现在就该投递
    Deliver,
    /// 暂停投递，空闲到这个时刻。空闲期照常自动回复未读
    Idle(DateTime<Local>),
    /// 已过设定的结束时刻，整个任务收工
    Finished,
}

/// 校验并规整一份计划。
///
/// 间隔为 0 会让周期退化成忙循环，结束时刻早于当前时刻则任务一启动就结束——
/// 两者都该在提交任务时就说清楚，而不是等跑起来才发现什么都没发生
pub fn resolve_plan(
    plan: Option<PeriodicPlan>,
    now: DateTime<Local>,
) -> Result<PeriodicPlan, String> {
    let Some(mut plan) = plan else {
        return Err("周期性投递缺少执行计划".to_string());
    };
    if plan.interval_minutes == 0 {
        return Err("周期性投递间隔必须大于 0 分钟".to_string());
    }
    if plan
        .windows
        .iter()
        .any(|window| window.start_minute > MINUTES_PER_DAY || window.end_minute > MINUTES_PER_DAY)
    {
        return Err("投递时段必须落在 00:00 - 24:00 之间".to_string());
    }
    plan.windows = normalize_windows(&plan.windows);
    if let Some(run_until) = plan.run_until {
        if run_until <= now {
            return Err("自动结束时间必须晚于当前时间".to_string());
        }
    }
    Ok(plan)
}

/// 规整一组投递时段：拆开跨零点的段、排序、合并重叠与首尾相接的段。
///
/// 覆盖全天时返回空——空在下游一律读作「不限制」，留着一个 00:00-24:00 的段
/// 只会让后面每一步都多判一次。
///
/// 相接的段也要合并：09:00-12:00 与 12:00-14:00 之间并没有真实的空档，
/// 分成两段会让「当前时段何时关闭」算出一个 12:00 的假边界，投递到点白停一次
pub fn normalize_windows(windows: &[DailyWindow]) -> Vec<DailyWindow> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    for window in windows {
        let start = window.start_minute.min(MINUTES_PER_DAY);
        let end = window.end_minute.min(MINUTES_PER_DAY);
        if start == end {
            // 单段「起止相同 = 全天」的语义在多段里同样成立：整组坍缩成不限制
            return Vec::new();
        }
        if start < end {
            spans.push((start, end));
        } else {
            // 跨零点的段拆成两截，之后就只剩普通区间要处理
            spans.push((start, MINUTES_PER_DAY));
            spans.push((0, end));
        }
    }

    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    if merged.as_slice() == [(0, MINUTES_PER_DAY)] {
        return Vec::new();
    }
    merged
        .into_iter()
        .map(|(start, end)| DailyWindow {
            start_minute: start,
            end_minute: end,
        })
        .collect()
}

/// 某个「零点起的分钟数」是否落在任意一段投递时段内。空表示全天可投
pub fn is_within_windows(windows: &[DailyWindow], minute_of_day: u32) -> bool {
    windows.is_empty()
        || windows
            .iter()
            .any(|window| is_within_window(window, minute_of_day))
}

/// 距离下一段投递时段开启的时刻，取所有段里最近的一个
pub fn next_windows_open(windows: &[DailyWindow], now: DateTime<Local>) -> DateTime<Local> {
    windows
        .iter()
        .map(|window| next_window_open(window, now))
        .min()
        .unwrap_or(now)
}

/// 当前所处那段时段的关闭时刻。不在任何一段内时返回 None
pub fn current_windows_close(
    windows: &[DailyWindow],
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    windows
        .iter()
        .find_map(|window| current_window_close(window, now))
}

/// 多段时段的可读描述，用于日志
pub fn describe_windows(windows: &[DailyWindow]) -> String {
    if windows.is_empty() {
        return "全天".to_string();
    }
    windows
        .iter()
        .map(describe_window)
        .collect::<Vec<_>>()
        .join("、")
}

/// 判断某个「零点起的分钟数」是否落在投递时段内
pub fn is_within_window(window: &DailyWindow, minute_of_day: u32) -> bool {
    if window.is_all_day() {
        return true;
    }
    if window.start_minute < window.end_minute {
        // 常规时段，例如 09:00-18:00
        minute_of_day >= window.start_minute && minute_of_day < window.end_minute
    } else {
        // 跨零点时段，例如 22:00-06:00
        minute_of_day >= window.start_minute || minute_of_day < window.end_minute
    }
}

/// 距离下一次窗口开启的时刻。已经在窗口内时返回当前时刻
pub fn next_window_open(window: &DailyWindow, now: DateTime<Local>) -> DateTime<Local> {
    if is_within_window(window, minute_of_day(now)) {
        return now;
    }
    at_minute_of_day(now, window.start_minute)
        .filter(|candidate| *candidate > now)
        .unwrap_or_else(|| {
            at_minute_of_day(now + ChronoDuration::days(1), window.start_minute)
                .unwrap_or(now + ChronoDuration::hours(1))
        })
}

/// 当前窗口的关闭时刻。不在窗口内时返回 None
pub fn current_window_close(window: &DailyWindow, now: DateTime<Local>) -> Option<DateTime<Local>> {
    if !is_within_window(window, minute_of_day(now)) {
        return None;
    }
    let today_close = at_minute_of_day(now, window.end_minute)?;
    if today_close > now {
        Some(today_close)
    } else {
        // 跨零点窗口里，终点落在「明天」那一侧
        at_minute_of_day(now + ChronoDuration::days(1), window.end_minute)
    }
}

/// 算出此刻该做什么
pub fn plan_state(plan: &PeriodicPlan, now: DateTime<Local>) -> PeriodicState {
    if plan.run_until.is_some_and(|deadline| now >= deadline) {
        return PeriodicState::Finished;
    }
    if is_within_windows(&plan.windows, minute_of_day(now)) {
        return PeriodicState::Deliver;
    }
    let open_at = next_windows_open(&plan.windows, now);
    // 窗口在结束时刻之后才开，说明这一觉睡过去就没有下一轮了，直接收工
    match plan.run_until {
        Some(deadline) if open_at >= deadline => PeriodicState::Finished,
        _ => PeriodicState::Idle(open_at),
    }
}

/// 一轮投递结束后，下一轮应该在什么时候开始。
///
/// 取「间隔到期」「窗口关闭」「任务结束」三者中最早的一个：窗口一关就该停下，
/// 哪怕间隔还没走完；结束时刻同理
pub fn next_delivery_at(plan: &PeriodicPlan, now: DateTime<Local>) -> DateTime<Local> {
    next_delivery_at_after(plan, now, plan.interval_minutes)
}

/// 同 [`next_delivery_at`]，但间隔由调用方给。
///
/// 拟人化会把用户设的间隔往上抖一点，抖出来的值只对这一轮有效，不能写回计划——
/// 计划是任务入队时固定下来的快照，改了它下一轮就会在抖过的值上再抖一次，
/// 几轮之后间隔会滚成一个离谱的数
pub fn next_delivery_at_after(
    plan: &PeriodicPlan,
    now: DateTime<Local>,
    interval_minutes: u64,
) -> DateTime<Local> {
    let mut target = now + ChronoDuration::minutes(interval_minutes.min(i64::MAX as u64) as i64);
    if let Some(close_at) = current_windows_close(&plan.windows, now) {
        target = target.min(close_at);
    }
    if let Some(deadline) = plan.run_until {
        target = target.min(deadline);
    }
    target
}

/// 距离目标时刻还有多少秒，已经过点时返回 0
pub fn seconds_until(target: DateTime<Local>, now: DateTime<Local>) -> u64 {
    (target - now).num_seconds().max(0) as u64
}

/// 把「零点起的分钟数」格式化成 `HH:MM`，用于日志
pub fn format_minute_of_day(minute: u32) -> String {
    let minute = minute.min(MINUTES_PER_DAY);
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// 投递时段的可读描述，用于日志
pub fn describe_window(window: &DailyWindow) -> String {
    format!(
        "{}-{}",
        format_minute_of_day(window.start_minute),
        format_minute_of_day(window.end_minute)
    )
}

/// 撞上平台每日沟通上限之后，打招呼会一条接一条地失败，而岗位列表还能一直往下滚。
/// 没有这个熔断，本轮就会在毫无产出的情况下把整个投递窗口耗光
pub const DEFAULT_MAX_CONSECUTIVE_GREET_FAILURES: u32 = 5;

/// 一轮投递的预算。
///
/// 这是「一直在投递、回复轮不上」的正解：岗位列表几乎是无限的，一轮不设上界
/// 就可能跑几个小时，两轮之间的空闲期自然永远轮不到。字段取 0 一律表示不限，
/// 「单轮自动求职」走 [`RoundBudget::unlimited`]，行为与改造前一致
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundBudget {
    /// 本轮最多打招呼多少条，0 表示不限
    pub max_greets: u32,
    /// 本轮最长跑多少分钟，0 表示不限
    pub max_minutes: u64,
    /// 连续多少次打招呼失败即熔断，0 表示不熔断
    pub max_consecutive_greet_failures: u32,
}

/// 预算检查的结论
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetVerdict {
    /// 预算还够，继续处理下一个岗位
    Continue,
    /// 打招呼条数达到上限
    GreetLimit,
    /// 本轮耗时达到上限
    TimeLimit,
    /// 连续打招呼失败达到熔断阈值
    FailureLimit,
}

impl BudgetVerdict {
    pub fn is_exhausted(self) -> bool {
        !matches!(self, Self::Continue)
    }
}

impl RoundBudget {
    /// 不设任何上界。改造前的单轮投递就是这个形态
    pub fn unlimited() -> Self {
        Self {
            max_greets: 0,
            max_minutes: 0,
            max_consecutive_greet_failures: 0,
        }
    }

    pub fn from_plan(plan: &PeriodicPlan) -> Self {
        Self {
            max_greets: plan.max_greets_per_round,
            max_minutes: plan.max_round_minutes,
            max_consecutive_greet_failures: DEFAULT_MAX_CONSECUTIVE_GREET_FAILURES,
        }
    }

    /// 本轮是否还能继续。
    ///
    /// 时间由调用方以「本轮已跑了多久」的形式传入，保持这里可测
    pub fn check(
        &self,
        greeted: u32,
        elapsed: std::time::Duration,
        consecutive_failures: u32,
    ) -> BudgetVerdict {
        if self.max_greets > 0 && greeted >= self.max_greets {
            return BudgetVerdict::GreetLimit;
        }
        if self.max_minutes > 0 && elapsed.as_secs() >= self.max_minutes.saturating_mul(60) {
            return BudgetVerdict::TimeLimit;
        }
        if self.max_consecutive_greet_failures > 0
            && consecutive_failures >= self.max_consecutive_greet_failures
        {
            return BudgetVerdict::FailureLimit;
        }
        BudgetVerdict::Continue
    }
}

fn minute_of_day(moment: DateTime<Local>) -> u32 {
    moment.hour() * 60 + moment.minute()
}

/// 把某一天的日期与「零点起的分钟数」拼成具体时刻。
///
/// 夏令时切换那天可能不存在对应的本地时刻，此时返回 None 交由调用方兜底
fn at_minute_of_day(day: DateTime<Local>, minute: u32) -> Option<DateTime<Local>> {
    // 24:00 指的是次日零点，NaiveTime 表示不了，换算成「明天 00:00」
    if minute >= MINUTES_PER_DAY {
        return at_minute_of_day(day + ChronoDuration::days(1), 0);
    }
    let time = NaiveTime::from_hms_opt(minute / 60, minute % 60, 0)?;
    Local
        .from_local_datetime(&day.date_naive().and_time(time))
        .earliest()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Local)
    }

    fn local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    fn window(start: u32, end: u32) -> DailyWindow {
        DailyWindow::new(start, end)
    }

    #[test]
    fn regular_window_covers_only_its_own_range() {
        let daytime = window(9 * 60, 18 * 60);

        assert!(is_within_window(&daytime, 9 * 60));
        assert!(is_within_window(&daytime, 17 * 60 + 59));
        assert!(!is_within_window(&daytime, 18 * 60));
        assert!(!is_within_window(&daytime, 8 * 60 + 59));
    }

    /// 起止相同表达的是「不限制」。当成空区间会让投递彻底停摆，
    /// 用户看到的是任务在跑却什么都不做，比任何报错都难排查
    #[test]
    fn identical_bounds_mean_all_day_rather_than_never() {
        let all_day = window(9 * 60, 9 * 60);

        assert!(is_within_window(&all_day, 3 * 60));
        assert!(is_within_window(&all_day, 15 * 60));
    }

    #[test]
    fn window_crossing_midnight_wraps_correctly() {
        let night = window(22 * 60, 6 * 60);

        assert!(is_within_window(&night, 23 * 60));
        assert!(is_within_window(&night, 2 * 60));
        assert!(!is_within_window(&night, 12 * 60));
    }

    #[test]
    fn half_hour_boundaries_are_representable() {
        let window = window(9 * 60 + 30, 18 * 60 + 30);

        assert!(!is_within_window(&window, 9 * 60 + 29));
        assert!(is_within_window(&window, 9 * 60 + 30));
        assert!(is_within_window(&window, 18 * 60 + 29));
        assert!(!is_within_window(&window, 18 * 60 + 30));
    }

    #[test]
    fn next_window_open_returns_now_when_already_inside() {
        let now = local(2026, 8, 19, 10, 0);

        assert_eq!(next_window_open(&window(9 * 60, 18 * 60), now), now);
    }

    #[test]
    fn next_window_open_picks_today_when_the_window_has_not_started() {
        let now = local(2026, 8, 19, 7, 0);

        assert_eq!(
            next_window_open(&window(9 * 60, 18 * 60), now),
            local(2026, 8, 19, 9, 0)
        );
    }

    #[test]
    fn next_window_open_rolls_over_to_tomorrow_after_the_window_closed() {
        let now = local(2026, 8, 19, 20, 0);

        assert_eq!(
            next_window_open(&window(9 * 60, 18 * 60), now),
            local(2026, 8, 20, 9, 0)
        );
    }

    #[test]
    fn current_window_close_is_none_outside_the_window() {
        let now = local(2026, 8, 19, 20, 0);

        assert_eq!(current_window_close(&window(9 * 60, 18 * 60), now), None);
    }

    /// 跨零点窗口的终点落在「明天」那一侧，按当天日期拼出来的时刻已经过去了
    #[test]
    fn current_window_close_of_a_midnight_crossing_window_lands_on_the_next_day() {
        let now = local(2026, 8, 19, 23, 0);

        assert_eq!(
            current_window_close(&window(22 * 60, 6 * 60), now),
            Some(local(2026, 8, 20, 6, 0))
        );
    }

    /// 午休那两小时正是多段存在的理由：09-12 与 14-18 中间必须真的停下来
    #[test]
    fn two_windows_leave_the_gap_between_them_uncovered() {
        let split = normalize_windows(&[window(9 * 60, 12 * 60), window(14 * 60, 18 * 60)]);

        assert_eq!(split.len(), 2);
        assert!(is_within_windows(&split, 11 * 60));
        assert!(!is_within_windows(&split, 13 * 60));
        assert!(is_within_windows(&split, 15 * 60));
        assert!(!is_within_windows(&split, 20 * 60));
    }

    #[test]
    fn windows_are_sorted_regardless_of_input_order() {
        let sorted = normalize_windows(&[window(14 * 60, 18 * 60), window(9 * 60, 12 * 60)]);

        assert_eq!(sorted[0].start_minute, 9 * 60);
        assert_eq!(sorted[1].start_minute, 14 * 60);
    }

    /// 相接的两段之间没有真实的空档。留成两段会让「当前时段何时关闭」
    /// 算出一个 12:00 的假边界，投递到点白停一次
    #[test]
    fn touching_windows_are_merged_into_one() {
        let merged = normalize_windows(&[window(9 * 60, 12 * 60), window(12 * 60, 14 * 60)]);

        assert_eq!(merged, vec![window(9 * 60, 14 * 60)]);
    }

    #[test]
    fn overlapping_windows_are_merged_into_one() {
        let merged = normalize_windows(&[window(9 * 60, 13 * 60), window(11 * 60, 18 * 60)]);

        assert_eq!(merged, vec![window(9 * 60, 18 * 60)]);
    }

    /// 跨零点的段拆成两截之后，下游就只剩 start < end 的普通区间要处理
    #[test]
    fn a_midnight_crossing_window_is_split_into_two_spans() {
        let split = normalize_windows(&[window(22 * 60, 6 * 60)]);

        assert_eq!(split, vec![window(0, 6 * 60), window(22 * 60, MINUTES_PER_DAY)]);
        assert!(is_within_windows(&split, 23 * 60));
        assert!(is_within_windows(&split, 2 * 60));
        assert!(!is_within_windows(&split, 12 * 60));
    }

    /// 覆盖满一整天等于没有限制，留着一个 00:00-24:00 的段只会让下游每步多判一次
    #[test]
    fn windows_covering_the_whole_day_collapse_to_unrestricted() {
        assert!(normalize_windows(&[window(0, MINUTES_PER_DAY)]).is_empty());
        assert!(normalize_windows(&[window(0, 12 * 60), window(12 * 60, MINUTES_PER_DAY)]).is_empty());
        assert!(normalize_windows(&[window(9 * 60, 9 * 60)]).is_empty());
        assert!(normalize_windows(&[]).is_empty());
    }

    /// 处在前一段里时，下一轮的边界是这一段的终点，不是最后一段的终点
    #[test]
    fn next_delivery_is_capped_by_the_window_the_moment_falls_in() {
        let plan = PeriodicPlan {
            windows: normalize_windows(&[window(9 * 60, 12 * 60), window(14 * 60, 18 * 60)]),
            ..PeriodicPlan::every(120)
        };

        assert_eq!(
            next_delivery_at(&plan, local(2026, 8, 19, 11, 0)),
            local(2026, 8, 19, 12, 0)
        );
        assert_eq!(
            next_delivery_at(&plan, local(2026, 8, 19, 17, 0)),
            local(2026, 8, 19, 18, 0)
        );
    }

    /// 午休期间要醒在 14:00，而不是睡到次日 09:00
    #[test]
    fn the_gap_between_windows_wakes_at_the_next_window_not_tomorrow() {
        let plan = PeriodicPlan {
            windows: normalize_windows(&[window(9 * 60, 12 * 60), window(14 * 60, 18 * 60)]),
            ..PeriodicPlan::every(30)
        };

        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 13, 0)),
            PeriodicState::Idle(local(2026, 8, 19, 14, 0))
        );
        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 20, 0)),
            PeriodicState::Idle(local(2026, 8, 20, 9, 0))
        );
    }

    #[test]
    fn multiple_windows_are_described_in_wall_clock_time() {
        assert_eq!(describe_windows(&[]), "全天");
        assert_eq!(
            describe_windows(&[window(9 * 60, 12 * 60), window(14 * 60, 18 * 60)]),
            "09:00-12:00、14:00-18:00"
        );
    }

    #[test]
    fn a_plan_without_a_window_always_delivers() {
        let plan = PeriodicPlan::every(30);

        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 3, 0)),
            PeriodicState::Deliver
        );
    }

    #[test]
    fn a_plan_outside_its_window_idles_until_the_window_opens() {
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 18 * 60)],
            ..PeriodicPlan::every(30)
        };

        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 20, 0)),
            PeriodicState::Idle(local(2026, 8, 20, 9, 0))
        );
    }

    #[test]
    fn a_plan_past_its_deadline_is_finished() {
        let plan = PeriodicPlan {
            run_until: Some(local(2026, 8, 19, 18, 0)),
            ..PeriodicPlan::every(30)
        };

        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 18, 0)),
            PeriodicState::Finished
        );
        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 17, 59)),
            PeriodicState::Deliver
        );
    }

    /// 下一次窗口开启已经在结束时刻之后，再睡过去也没有下一轮可投，
    /// 与其让任务空等一夜再退出，不如当场收工
    #[test]
    fn a_plan_whose_window_reopens_after_the_deadline_finishes_instead_of_idling() {
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 18 * 60)],
            run_until: Some(local(2026, 8, 20, 2, 0)),
            ..PeriodicPlan::every(30)
        };

        assert_eq!(
            plan_state(&plan, local(2026, 8, 19, 20, 0)),
            PeriodicState::Finished
        );
    }

    #[test]
    fn next_delivery_follows_the_interval_when_nothing_else_binds() {
        let plan = PeriodicPlan::every(30);
        let now = local(2026, 8, 19, 10, 0);

        assert_eq!(next_delivery_at(&plan, now), local(2026, 8, 19, 10, 30));
    }

    /// 窗口一关就该停下，哪怕间隔还没走完——否则最后一轮会溢出到时段之外
    #[test]
    fn next_delivery_is_capped_by_the_window_close() {
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 18 * 60)],
            ..PeriodicPlan::every(120)
        };
        let now = local(2026, 8, 19, 17, 0);

        assert_eq!(next_delivery_at(&plan, now), local(2026, 8, 19, 18, 0));
    }

    /// 拟人化抖过的间隔只作用于这一轮，窗口与结束时刻照样把它压回来
    #[test]
    fn an_overridden_interval_is_still_capped_by_the_window_and_deadline() {
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 18 * 60)],
            ..PeriodicPlan::every(30)
        };
        let now = local(2026, 8, 19, 10, 0);

        assert_eq!(
            next_delivery_at_after(&plan, now, 37),
            local(2026, 8, 19, 10, 37)
        );
        assert_eq!(
            next_delivery_at_after(&plan, local(2026, 8, 19, 17, 50), 37),
            local(2026, 8, 19, 18, 0)
        );
    }

    #[test]
    fn next_delivery_is_capped_by_the_deadline() {
        let plan = PeriodicPlan {
            run_until: Some(local(2026, 8, 19, 10, 10)),
            ..PeriodicPlan::every(120)
        };
        let now = local(2026, 8, 19, 10, 0);

        assert_eq!(next_delivery_at(&plan, now), local(2026, 8, 19, 10, 10));
    }

    #[test]
    fn resolve_plan_rejects_a_missing_or_degenerate_plan() {
        let now = local(2026, 8, 19, 10, 0);

        assert!(resolve_plan(None, now).is_err());
        assert!(resolve_plan(Some(PeriodicPlan::every(0)), now).is_err());
    }

    #[test]
    fn resolve_plan_rejects_a_deadline_in_the_past() {
        let now = local(2026, 8, 19, 10, 0);
        let plan = PeriodicPlan {
            run_until: Some(local(2026, 8, 19, 9, 0)),
            ..PeriodicPlan::every(30)
        };

        assert!(resolve_plan(Some(plan), now).is_err());
    }

    /// 全天窗口留在计划里只会让后面每一步都多判一次，规整阶段直接摘掉
    #[test]
    fn resolve_plan_drops_an_all_day_window() {
        let now = local(2026, 8, 19, 10, 0);
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 9 * 60)],
            ..PeriodicPlan::every(30)
        };

        assert!(resolve_plan(Some(plan), now).unwrap().windows.is_empty());
    }

    #[test]
    fn seconds_until_never_goes_negative() {
        let now = local(2026, 8, 19, 10, 0);

        assert_eq!(seconds_until(local(2026, 8, 19, 10, 1), now), 60);
        assert_eq!(seconds_until(local(2026, 8, 19, 9, 0), now), 0);
    }

    #[test]
    fn window_is_described_in_wall_clock_time() {
        assert_eq!(format_minute_of_day(9 * 60 + 30), "09:30");
        assert_eq!(format_minute_of_day(0), "00:00");
        assert_eq!(format_minute_of_day(MINUTES_PER_DAY), "24:00");
        assert_eq!(describe_window(&window(9 * 60, 18 * 60)), "09:00-18:00");
    }

    /// 前端只传间隔时，反序列化出来的计划必须等价于改造前的行为
    #[test]
    fn a_plan_json_with_only_an_interval_deserializes_to_the_legacy_behaviour() {
        let plan: PeriodicPlan = serde_json::from_str(r#"{"interval_minutes":30}"#).unwrap();

        assert_eq!(plan, PeriodicPlan::every(30));
    }

    #[test]
    fn an_unlimited_budget_never_stops_a_round() {
        let budget = RoundBudget::unlimited();

        assert_eq!(
            budget.check(10_000, std::time::Duration::from_secs(86_400), 999),
            BudgetVerdict::Continue
        );
    }

    #[test]
    fn a_budget_stops_the_round_once_the_greet_quota_is_spent() {
        let budget = RoundBudget {
            max_greets: 30,
            ..RoundBudget::unlimited()
        };

        assert_eq!(
            budget.check(29, std::time::Duration::ZERO, 0),
            BudgetVerdict::Continue
        );
        assert_eq!(
            budget.check(30, std::time::Duration::ZERO, 0),
            BudgetVerdict::GreetLimit
        );
    }

    #[test]
    fn a_budget_stops_the_round_once_it_runs_too_long() {
        let budget = RoundBudget {
            max_minutes: 60,
            ..RoundBudget::unlimited()
        };

        assert_eq!(
            budget.check(0, std::time::Duration::from_secs(3599), 0),
            BudgetVerdict::Continue
        );
        assert_eq!(
            budget.check(0, std::time::Duration::from_secs(3600), 0),
            BudgetVerdict::TimeLimit
        );
    }

    /// 撞上平台每日沟通上限后打招呼会连续失败，而列表还能一直往下滚——
    /// 没有这道熔断，本轮就会在零产出的情况下把整个投递窗口耗光
    #[test]
    fn a_budget_trips_on_consecutive_greet_failures() {
        let budget = RoundBudget::from_plan(&PeriodicPlan::every(30));

        assert_eq!(
            budget.check(0, std::time::Duration::ZERO, 4),
            BudgetVerdict::Continue
        );
        assert_eq!(
            budget.check(0, std::time::Duration::ZERO, 5),
            BudgetVerdict::FailureLimit
        );
    }

    #[test]
    fn a_budget_built_from_a_plan_carries_its_round_limits() {
        let plan = PeriodicPlan {
            max_greets_per_round: 12,
            max_round_minutes: 45,
            ..PeriodicPlan::every(30)
        };

        let budget = RoundBudget::from_plan(&plan);

        assert_eq!(budget.max_greets, 12);
        assert_eq!(budget.max_minutes, 45);
        assert!(BudgetVerdict::GreetLimit.is_exhausted());
        assert!(!BudgetVerdict::Continue.is_exhausted());
    }

    /// 前端用 `Date.toISOString()` 拼结束时刻，发过来的是 `Z` 结尾的 UTC 串，
    /// 而计划里存的是本地时刻。这条链路断掉的表现是任务提交时报「自动结束时间
    /// 必须晚于当前时间」——而用户明明选的是几小时之后
    #[test]
    fn a_utc_deadline_from_the_frontend_is_read_as_the_same_instant() {
        let plan: PeriodicPlan = serde_json::from_str(
            r#"{"interval_minutes":30,"run_until":"2026-08-19T10:00:00.000Z"}"#,
        )
        .unwrap();

        assert_eq!(plan.run_until, Some(at("2026-08-19T10:00:00+00:00")));
        assert_eq!(
            plan_state(&plan, at("2026-08-19T09:59:00+00:00")),
            PeriodicState::Deliver
        );
        assert_eq!(
            plan_state(&plan, at("2026-08-19T10:00:00+00:00")),
            PeriodicState::Finished
        );
    }

    #[test]
    fn a_plan_round_trips_through_json_with_its_deadline() {
        let plan = PeriodicPlan {
            windows: vec![window(9 * 60, 18 * 60)],
            run_until: Some(at("2026-08-21T02:00:00+08:00")),
            max_greets_per_round: 30,
            max_round_minutes: 60,
            ..PeriodicPlan::every(30)
        };

        let encoded = serde_json::to_string(&plan).unwrap();
        let decoded: PeriodicPlan = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, plan);
    }
}
