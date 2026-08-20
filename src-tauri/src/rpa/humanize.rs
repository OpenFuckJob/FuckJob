//! 拟人化：把既有的节奏参数换算成「今天这个人」的操作习惯。
//!
//! 和 [`super::polling`]、[`super::schedule`] 一样全是纯函数，随机源与当前日期
//! 一律由调用方传入。这里算错不会崩，只会让机器特征重新暴露出来——每条投递
//! 都隔 4 秒、每轮都正好 30 条、连投两小时不喘气——而这些症状在集成测试里
//! 根本看不出来，只能靠单测把边界钉死。
//!
//! # 为什么不新开一套参数
//!
//! 用户在配置页设的「单轮 30 条、间隔 30 分钟」表达的是投递意图，不是动作节拍。
//! 拟人化要改的是后者：意图仍是 30 条那个量级，但今天可能 26 条、明天 33 条，
//! 中途还会停下来歇几分钟。所以这里没有「休息阈值」「休息时长」这类新旋钮，
//! 全部由 [`Persona`] 从既有配置派生。
//!
//! # 稳定随机
//!
//! 人格种子存在配置里长期不变，每天再由 `(种子, 日期)` 派生出当天的具体策略。
//! 于是同一天内行为自洽——手速、休息习惯是一以贯之的；换一天自动换一套；
//! 不同用户之间也各不相同。全程确定性推导，给定种子和日期就能复现，可单测。

use chrono::{Datelike, NaiveDate};

use crate::config::{HumanizeConfig, HumanizeIntensity};
use crate::rpa::schedule::RoundBudget;

/// 单轮上限设成「不限」时，休息节奏所依据的基准条数。
///
/// 不限不等于「一口气投到天亮」——那恰恰是最该拦住的模式。没有用户给的量级时
/// 按这个数推导休息节奏
pub const FALLBACK_BASE_GREETS: u32 = 30;

/// 当日人格：一套当天固定、跨天自动更换的操作习惯。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Persona {
    pub intensity: HumanizeIntensity,
    /// 今日整体节奏系数。小于 1 手快，大于 1 手慢
    pub pace: f64,
    /// 连投多少条之后歇一会儿
    pub break_after_greets: u32,
    /// 每次休息的时长区间（秒）
    pub break_seconds: (u64, u64),
    /// 两条投递之间的基础停顿区间（毫秒）
    pub greet_gap_ms: (u64, u64),
    /// 停顿时走神的概率，以及走神时额外拖长的秒数区间
    pub distraction_chance: f64,
    pub distraction_seconds: (u64, u64),
    /// 岗位通过筛选后仍然「只看不投」的概率
    pub skim_rate: f64,
    /// 打字速度（字符/分钟）
    pub typing_cpm: u32,
    /// 单轮预算与投递间隔的抖动幅度（±比例）
    pub budget_jitter: f64,
}

/// 一个强度档位对应的扰动配方。各区间的下界即该档位最轻的表现
#[derive(Debug, Clone, Copy)]
struct IntensitySpec {
    pace: (f64, f64),
    /// 休息阈值相对基准条数的比例区间
    break_after_ratio: (f64, f64),
    break_seconds: (u64, u64),
    /// 基础停顿相对既有 3-5 秒节奏的倍率区间
    gap_scale: (f64, f64),
    distraction_chance: (f64, f64),
    distraction_seconds: (u64, u64),
    skim_rate: (f64, f64),
    typing_cpm: (u32, u32),
    budget_jitter: f64,
}

/// 改造前投递循环里那段固定的 3-5 秒停顿。拟人化在它之上缩放，
/// 而不是另起一套毫秒数——关掉拟人化时行为要能原样退回这里
const BASE_GREET_GAP_MS: (u64, u64) = (3_000, 5_000);

impl HumanizeIntensity {
    fn spec(self) -> IntensitySpec {
        match self {
            // 只把直线掰弯一点，产出基本不受影响
            HumanizeIntensity::Light => IntensitySpec {
                pace: (0.9, 1.15),
                break_after_ratio: (0.75, 1.0),
                break_seconds: (60, 180),
                gap_scale: (0.9, 1.4),
                distraction_chance: (0.02, 0.05),
                distraction_seconds: (8, 25),
                skim_rate: (0.0, 0.03),
                typing_cpm: (260, 400),
                budget_jitter: 0.10,
            },
            // 群里说的「投 30 条歇 5-10 分钟」就是这一档
            HumanizeIntensity::Standard => IntensitySpec {
                pace: (1.0, 1.5),
                break_after_ratio: (0.4, 0.75),
                break_seconds: (300, 600),
                gap_scale: (1.1, 2.2),
                distraction_chance: (0.05, 0.1),
                distraction_seconds: (15, 60),
                skim_rate: (0.04, 0.12),
                typing_cpm: (160, 260),
                budget_jitter: 0.25,
            },
            // 已经被限制过、或者账号很重要时用，宁可少投
            HumanizeIntensity::Cautious => IntensitySpec {
                pace: (1.3, 2.2),
                break_after_ratio: (0.25, 0.5),
                break_seconds: (600, 1_500),
                gap_scale: (1.8, 3.6),
                distraction_chance: (0.1, 0.2),
                distraction_seconds: (30, 120),
                skim_rate: (0.12, 0.25),
                typing_cpm: (100, 180),
                budget_jitter: 0.35,
            },
        }
    }
}

impl Persona {
    /// 从种子和日期推出当天的人格。关闭拟人化或没有种子时返回 None。
    ///
    /// `base_greets` 是用户设的单轮上限，取 0（不限）时回落到
    /// [`FALLBACK_BASE_GREETS`]——休息节奏总得有个量级可依
    pub fn derive(config: &HumanizeConfig, day: NaiveDate, base_greets: u32) -> Option<Self> {
        if !config.enabled || config.persona_seed == 0 {
            return None;
        }
        let spec = config.intensity.spec();
        let base = if base_greets == 0 {
            FALLBACK_BASE_GREETS
        } else {
            base_greets
        };

        // 种子与日期揉在一起：同一天恒定同一套，跨天自动翻新
        let mut rng = Rng::new(config.persona_seed ^ day_key(day));
        let pace = rng.range(spec.pace.0, spec.pace.1);
        let break_ratio = rng.range(spec.break_after_ratio.0, spec.break_after_ratio.1);
        let gap_scale = rng.range(spec.gap_scale.0, spec.gap_scale.1);

        Some(Self {
            intensity: config.intensity,
            pace,
            // 至少 3 条一歇：再密就成了「投一条歇一次」，那不是人在用，是程序在装模作样
            break_after_greets: ((base as f64 * break_ratio).round() as u32).max(3),
            break_seconds: scale_span(spec.break_seconds, pace),
            greet_gap_ms: scale_span(BASE_GREET_GAP_MS, gap_scale),
            distraction_chance: rng.range(spec.distraction_chance.0, spec.distraction_chance.1),
            distraction_seconds: spec.distraction_seconds,
            skim_rate: rng.range(spec.skim_rate.0, spec.skim_rate.1),
            typing_cpm: rng
                .range(spec.typing_cpm.0 as f64, spec.typing_cpm.1 as f64)
                .round()
                .max(30.0) as u32,
            budget_jitter: spec.budget_jitter,
        })
    }

    /// 读当天日期推出人格，运行期用这个。
    pub fn today(config: &HumanizeConfig, base_greets: u32) -> Option<Self> {
        Self::derive(config, chrono::Local::now().date_naive(), base_greets)
    }

    /// 给本轮预算蒙一层抖动。
    ///
    /// 用户设的 30 条是量级不是配额——每轮都精确停在第 30 条，这个「精确」
    /// 本身就是特征。`roll` 取 0..=1
    pub fn shape_budget(&self, budget: RoundBudget, roll: f64) -> RoundBudget {
        RoundBudget {
            max_greets: jitter_u32(budget.max_greets, self.budget_jitter, roll).max(
                // 抖到 0 会被下游读成「不限」，本轮就彻底没有上界了
                if budget.max_greets > 0 { 1 } else { 0 },
            ),
            max_minutes: jitter_u64(budget.max_minutes, self.budget_jitter, 1.0 - roll),
            max_consecutive_greet_failures: budget.max_consecutive_greet_failures,
        }
    }

    /// 给两轮之间的间隔蒙一层抖动，并按今日节奏整体拉长。
    ///
    /// 间隔只向上抖：把用户设的 30 分钟抖成 22 分钟等于比他要求的更频繁，
    /// 拟人化没有资格替他提高投递密度
    pub fn shape_interval_minutes(&self, minutes: u64, roll: f64) -> u64 {
        if minutes == 0 {
            return 0;
        }
        let clamped = roll.clamp(0.0, 1.0);
        let factor = 1.0 + self.budget_jitter * clamped;
        ((minutes as f64 * factor).round() as u64).max(minutes)
    }

    /// 两条投递之间停多久（毫秒）。`gap_roll` 取区间内的位置，
    /// `distraction_roll` 决定这次要不要走神，`length_roll` 决定走神多久
    pub fn gap_after_greet_ms(
        &self,
        gap_roll: f64,
        distraction_roll: f64,
        length_roll: f64,
    ) -> u64 {
        let base = pick_u64(self.greet_gap_ms, gap_roll);
        if distraction_roll.clamp(0.0, 1.0) >= self.distraction_chance {
            return base;
        }
        // 真人会突然去回个消息、倒杯水，这种离群的长停顿正是固定节律做不出来的
        base.saturating_add(pick_u64(self.distraction_seconds, length_roll).saturating_mul(1_000))
    }

    /// 距上次休息已经投了这么多条，该歇了吗
    pub fn should_break(&self, greets_since_break: u32) -> bool {
        greets_since_break >= self.break_after_greets
    }

    /// 这次休息多久（秒）
    pub fn break_seconds(&self, roll: f64) -> u64 {
        pick_u64(self.break_seconds, roll)
    }

    /// 这个岗位要不要「只看不投」。
    ///
    /// 每个符合条件的岗位都投，本身就是人做不到的事——真人会挑、会犹豫、
    /// 会看完描述觉得不合适就走。跳过的岗位不入库，下一轮还会再遇到
    pub fn should_skim(&self, roll: f64) -> bool {
        roll.clamp(0.0, 1.0) < self.skim_rate
    }

    /// 敲一个字符要多久（毫秒）。`roll` 制造快慢不匀的手感
    pub fn typing_delay_ms(&self, roll: f64) -> u64 {
        let mean = 60_000.0 / self.typing_cpm.max(1) as f64;
        // 0.6~1.6 倍：匀速敲字比敲得慢更可疑
        let factor = 0.6 + roll.clamp(0.0, 1.0);
        (mean * factor).round().max(1.0) as u64
    }

    /// 打完一段字之后、按下发送之前的停顿（毫秒）。真人会回读一遍
    pub fn review_before_send_ms(&self, roll: f64) -> u64 {
        let span = (600, 2_600);
        ((pick_u64(span, roll) as f64) * self.pace).round() as u64
    }

    /// 鼠标从当前位置移到目标要走几步。步数越多轨迹越细腻，代价是 CDP 往返
    pub fn mouse_steps(&self, roll: f64) -> u32 {
        pick_u64((6, 14), roll) as u32
    }

    /// 一行摘要，给日志和配置页展示当前生效的策略用
    pub fn describe(&self) -> String {
        format!(
            "每投 {} 条歇 {}-{} 分钟，岗位间停顿 {:.1}-{:.1} 秒，跳过率 {:.0}%，打字 {} 字/分",
            self.break_after_greets,
            self.break_seconds.0 / 60,
            self.break_seconds.1.div_ceil(60),
            self.greet_gap_ms.0 as f64 / 1000.0,
            self.greet_gap_ms.1 as f64 / 1000.0,
            self.skim_rate * 100.0,
            self.typing_cpm,
        )
    }
}

/// 把日期压成一个参与种子混合的整数
fn day_key(day: NaiveDate) -> u64 {
    day.num_days_from_ce() as u64
}

/// 按系数缩放一个区间，同时保证下界不越过上界
fn scale_span(span: (u64, u64), factor: f64) -> (u64, u64) {
    let low = ((span.0 as f64) * factor).round() as u64;
    let high = ((span.1 as f64) * factor).round() as u64;
    (low.min(high), high.max(low))
}

/// 用 0..=1 的随机源在区间内取值
fn pick_u64(span: (u64, u64), roll: f64) -> u64 {
    let (low, high) = if span.0 <= span.1 {
        (span.0, span.1)
    } else {
        (span.1, span.0)
    };
    low + ((high - low) as f64 * roll.clamp(0.0, 1.0)).round() as u64
}

/// 在 `value` 上下 `ratio` 比例内抖动。0 表示「不限」，抖了也还是不限
fn jitter_u64(value: u64, ratio: f64, roll: f64) -> u64 {
    if value == 0 {
        return 0;
    }
    let offset = (roll.clamp(0.0, 1.0) * 2.0 - 1.0) * ratio;
    ((value as f64) * (1.0 + offset)).round().max(0.0) as u64
}

fn jitter_u32(value: u32, ratio: f64, roll: f64) -> u32 {
    jitter_u64(value as u64, ratio, roll).min(u32::MAX as u64) as u32
}

/// SplitMix64。选它是因为实现只有几行且与平台无关——
/// 人格必须能在任何机器上由同一组种子复现，否则单测钉不住
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        // 取高 53 位映射到 [0,1)，与 f64 的尾数宽度对齐
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
}

/// 运行期的随机源。人格是稳定的，落到每一次具体动作上仍要现掷
pub fn roll() -> f64 {
    use rand::Rng as _;
    rand::thread_rng().gen_range(0.0..=1.0)
}

thread_local! {
    /// 当前任务的人格。
    ///
    /// 鼠标轨迹、打字速度这些动作散落在整条调用链的最深处——岗位卡片点击、沟通
    /// 按钮、聊天输入框、滚动，每一处都在不同的函数里。为它们逐层加参数会把
    /// 「拟人化」这件事糊到几十个与之无关的签名上，而 RPA 全流程本来就跑在
    /// 一条专用线程上，和 [`crate::rpa::run_flow::is_job_task_stop_requested`]
    /// 用的是同一套线程内上下文
    static CURRENT_PERSONA: std::cell::RefCell<Option<Persona>> =
        const { std::cell::RefCell::new(None) };
}

/// 装上当前任务的人格，返回的守卫在离开作用域时自动摘下。
///
/// 必须用守卫而不是裸的 setter：任务提前返回、出错、被停止的路径有很多条，
/// 漏摘一次，下一个任务就会继承上一个任务的人格
#[must_use = "人格在守卫被丢弃时即摘下，忽略返回值等于没装"]
pub fn scoped_persona(persona: Option<Persona>) -> PersonaGuard {
    let previous = CURRENT_PERSONA.with(|slot| slot.replace(persona));
    PersonaGuard { previous }
}

pub struct PersonaGuard {
    previous: Option<Persona>,
}

impl Drop for PersonaGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_PERSONA.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// 当前线程正在使用的人格。拟人化关闭时返回 None，调用方据此走原有路径
pub fn current_persona() -> Option<Persona> {
    CURRENT_PERSONA.with(|slot| *slot.borrow())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(intensity: HumanizeIntensity) -> HumanizeConfig {
        HumanizeConfig {
            enabled: true,
            intensity,
            persona_seed: 0x5EED_1234_ABCD_0001,
        }
    }

    fn day(year: i32, month: u32, date: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, date).unwrap()
    }

    fn persona(intensity: HumanizeIntensity) -> Persona {
        Persona::derive(&config(intensity), day(2026, 8, 20), 30).unwrap()
    }

    /// 关掉开关必须彻底没有人格，投递循环据此走回改造前的固定节奏
    #[test]
    fn a_disabled_config_yields_no_persona() {
        let mut disabled = config(HumanizeIntensity::Standard);
        disabled.enabled = false;

        assert!(Persona::derive(&disabled, day(2026, 8, 20), 30).is_none());
    }

    /// 种子还没生成时同样没有人格，不能拿 0 当种子推出一套所有用户都一样的策略
    #[test]
    fn a_missing_seed_yields_no_persona() {
        let mut seedless = config(HumanizeIntensity::Standard);
        seedless.persona_seed = 0;

        assert!(Persona::derive(&seedless, day(2026, 8, 20), 30).is_none());
    }

    /// 「稳定随机」的字面意思：同一天同一种子，推几次都是同一套
    #[test]
    fn the_same_seed_and_day_always_derive_the_same_persona() {
        let config = config(HumanizeIntensity::Standard);

        let first = Persona::derive(&config, day(2026, 8, 20), 30).unwrap();
        let second = Persona::derive(&config, day(2026, 8, 20), 30).unwrap();

        assert_eq!(first, second);
    }

    /// 跨天自动换一套。每天都一模一样的话，长期看仍是一条直线
    #[test]
    fn a_new_day_derives_a_different_persona() {
        let config = config(HumanizeIntensity::Standard);

        let today = Persona::derive(&config, day(2026, 8, 20), 30).unwrap();
        let tomorrow = Persona::derive(&config, day(2026, 8, 21), 30).unwrap();

        assert_ne!(today, tomorrow);
    }

    /// 两个用户同一天也该各有各的习惯
    #[test]
    fn different_seeds_derive_different_personas() {
        let mut other = config(HumanizeIntensity::Standard);
        other.persona_seed = 0xA11C_E000_0000_0002;

        let mine = Persona::derive(&config(HumanizeIntensity::Standard), day(2026, 8, 20), 30);
        let theirs = Persona::derive(&other, day(2026, 8, 20), 30);

        assert_ne!(mine.unwrap(), theirs.unwrap());
    }

    /// 休息阈值必须从用户设的单轮上限派生，而不是写死一个 30
    #[test]
    fn the_break_threshold_scales_with_the_users_own_round_limit() {
        let config = config(HumanizeIntensity::Standard);

        let small = Persona::derive(&config, day(2026, 8, 20), 20).unwrap();
        let large = Persona::derive(&config, day(2026, 8, 20), 100).unwrap();

        assert!(large.break_after_greets > small.break_after_greets);
        assert!(small.break_after_greets <= 20);
    }

    /// 单轮不限时也得有个休息节奏，否则「不限」就成了通宵直投
    #[test]
    fn an_unlimited_round_still_gets_a_break_rhythm() {
        let persona = Persona::derive(&config(HumanizeIntensity::Standard), day(2026, 8, 20), 0);

        let persona = persona.unwrap();
        assert!(persona.break_after_greets >= 3);
        assert!(persona.break_after_greets <= FALLBACK_BASE_GREETS);
    }

    /// 阈值再小也不能小到「投一条歇一次」——那比匀速投递还反常
    #[test]
    fn the_break_threshold_never_collapses_to_every_single_greet() {
        for base in [1, 2, 3, 5] {
            let persona =
                Persona::derive(&config(HumanizeIntensity::Cautious), day(2026, 8, 20), base)
                    .unwrap();
            assert!(persona.break_after_greets >= 3, "base={base}");
        }
    }

    /// 档位越谨慎，歇得越勤、停顿越长、跳过越多
    #[test]
    fn a_more_cautious_intensity_slows_everything_down() {
        let light = persona(HumanizeIntensity::Light);
        let cautious = persona(HumanizeIntensity::Cautious);

        assert!(cautious.break_after_greets < light.break_after_greets);
        assert!(cautious.greet_gap_ms.0 > light.greet_gap_ms.0);
        assert!(cautious.skim_rate > light.skim_rate);
        assert!(cautious.typing_cpm < light.typing_cpm);
    }

    /// 单轮上限被抖动之后仍在同一量级，而不是被改成另一个数
    #[test]
    fn the_round_budget_is_nudged_rather_than_replaced() {
        let persona = persona(HumanizeIntensity::Standard);
        let budget = RoundBudget {
            max_greets: 30,
            max_minutes: 60,
            max_consecutive_greet_failures: 5,
        };

        let low = persona.shape_budget(budget, 0.0);
        let high = persona.shape_budget(budget, 1.0);

        assert!(low.max_greets < 30 && low.max_greets >= 20);
        assert!(high.max_greets > 30 && high.max_greets <= 40);
        assert_eq!(low.max_consecutive_greet_failures, 5);
    }

    /// 抖到 0 会被下游读成「不限」，本轮就彻底没有上界了——这是最坏的方向
    #[test]
    fn a_capped_budget_never_jitters_down_into_unlimited() {
        let mut persona = persona(HumanizeIntensity::Cautious);
        persona.budget_jitter = 5.0;
        let budget = RoundBudget {
            max_greets: 1,
            max_minutes: 1,
            max_consecutive_greet_failures: 5,
        };

        let shaped = persona.shape_budget(budget, 0.0);

        assert!(shaped.max_greets >= 1);
    }

    /// 本来就不限的预算抖完还是不限，不能凭空长出一个上界
    #[test]
    fn an_unlimited_budget_stays_unlimited_after_shaping() {
        let persona = persona(HumanizeIntensity::Standard);

        let shaped = persona.shape_budget(RoundBudget::unlimited(), 0.5);

        assert_eq!(shaped, RoundBudget::unlimited());
    }

    /// 间隔只向上抖：把 30 分钟抖成 22 分钟等于替用户提高了投递密度
    #[test]
    fn the_delivery_interval_only_ever_stretches() {
        let persona = persona(HumanizeIntensity::Standard);

        for roll in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(persona.shape_interval_minutes(30, roll) >= 30, "roll={roll}");
        }
        assert!(persona.shape_interval_minutes(30, 1.0) > 30);
        assert_eq!(persona.shape_interval_minutes(0, 1.0), 0);
    }

    /// 停顿落在人格给的区间内，走神时才会突破上界
    #[test]
    fn the_gap_between_greets_stays_in_range_unless_distracted() {
        let persona = persona(HumanizeIntensity::Standard);

        let calm = persona.gap_after_greet_ms(0.5, 1.0, 0.5);
        assert!(calm >= persona.greet_gap_ms.0 && calm <= persona.greet_gap_ms.1);

        let distracted = persona.gap_after_greet_ms(0.5, 0.0, 1.0);
        assert!(distracted > persona.greet_gap_ms.1);
    }

    /// 走神概率为 0 时永远不该被拖长
    #[test]
    fn a_zero_distraction_chance_never_stretches_the_gap() {
        let mut persona = persona(HumanizeIntensity::Standard);
        persona.distraction_chance = 0.0;

        let gap = persona.gap_after_greet_ms(0.0, 0.0, 1.0);

        assert_eq!(gap, persona.greet_gap_ms.0);
    }

    #[test]
    fn breaks_trigger_only_after_the_threshold_is_reached() {
        let persona = persona(HumanizeIntensity::Standard);
        let threshold = persona.break_after_greets;

        assert!(!persona.should_break(threshold - 1));
        assert!(persona.should_break(threshold));
        assert!(persona.should_break(threshold + 1));
    }

    #[test]
    fn break_length_stays_within_the_personas_range() {
        let persona = persona(HumanizeIntensity::Standard);

        assert_eq!(persona.break_seconds(0.0), persona.break_seconds.0);
        assert_eq!(persona.break_seconds(1.0), persona.break_seconds.1);
        // 群里提的「歇 5-10 分钟」正是标准档该给出的量级
        assert!(persona.break_seconds.0 >= 240);
        assert!(persona.break_seconds.1 <= 1_200);
    }

    /// 跳过率是概率，不是配额：随机源低于它才跳
    #[test]
    fn skimming_follows_the_configured_rate() {
        let mut persona = persona(HumanizeIntensity::Standard);
        persona.skim_rate = 0.1;

        assert!(persona.should_skim(0.05));
        assert!(!persona.should_skim(0.1));
        assert!(!persona.should_skim(0.9));
    }

    /// 跳过率为 0 时一个都不该跳，轻度档不能悄悄吃掉产出
    #[test]
    fn a_zero_skim_rate_never_skips() {
        let mut persona = persona(HumanizeIntensity::Light);
        persona.skim_rate = 0.0;

        for roll in [0.0, 0.001, 0.5, 1.0] {
            assert!(!persona.should_skim(roll), "roll={roll}");
        }
    }

    /// 匀速敲字比敲得慢更可疑，每个字符的耗时必须有波动
    #[test]
    fn typing_delay_varies_around_the_configured_speed() {
        let persona = persona(HumanizeIntensity::Standard);

        let fast = persona.typing_delay_ms(0.0);
        let slow = persona.typing_delay_ms(1.0);

        assert!(fast > 0);
        assert!(slow > fast);
    }

    /// 越界的随机源不该把等待放大到离谱的量级，也不该 panic
    #[test]
    fn out_of_range_rolls_are_clamped_everywhere() {
        let persona = persona(HumanizeIntensity::Standard);

        assert_eq!(persona.break_seconds(-3.0), persona.break_seconds.0);
        assert_eq!(persona.break_seconds(9.0), persona.break_seconds.1);
        assert_eq!(
            persona.gap_after_greet_ms(-1.0, 1.0, 0.0),
            persona.greet_gap_ms.0
        );
        assert!(persona.shape_interval_minutes(30, -2.0) >= 30);
        assert!(persona.typing_delay_ms(42.0) > 0);
    }

    #[test]
    fn the_persona_summary_reads_as_a_concrete_strategy() {
        let summary = persona(HumanizeIntensity::Standard).describe();

        assert!(summary.contains("条歇"));
        assert!(summary.contains("跳过率"));
        assert!(summary.contains("字/分"));
    }

    /// 随机源必须落在 0..=1，下游所有 clamp 都以此为前提
    #[test]
    fn the_runtime_random_source_stays_in_the_unit_interval() {
        for _ in 0..64 {
            let value = roll();
            assert!((0.0..=1.0).contains(&value), "{value}");
        }
    }

    /// 没装人格时深处的动作必须看得出来「现在不该拟人」
    #[test]
    fn no_persona_is_installed_by_default() {
        assert!(current_persona().is_none());
    }

    /// 守卫离开作用域就该摘干净，否则下一个任务会继承上一个任务的人格
    #[test]
    fn the_persona_guard_restores_the_previous_value() {
        let persona = persona(HumanizeIntensity::Standard);

        {
            let _guard = scoped_persona(Some(persona));
            assert_eq!(current_persona(), Some(persona));

            // 嵌套安装同样要能原样退回外层，而不是退成 None
            let inner = Persona::derive(&config(HumanizeIntensity::Cautious), day(2026, 8, 21), 30);
            {
                let _inner_guard = scoped_persona(inner);
                assert_eq!(current_persona(), inner);
            }
            assert_eq!(current_persona(), Some(persona));
        }

        assert!(current_persona().is_none());
    }

    #[test]
    fn the_seeded_generator_spreads_over_the_unit_interval() {
        let mut rng = Rng::new(7);
        let mut low = 0;
        let mut high = 0;

        for _ in 0..256 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value), "{value}");
            if value < 0.5 {
                low += 1;
            } else {
                high += 1;
            }
        }

        assert!(low > 64 && high > 64, "low={low} high={high}");
    }
}
