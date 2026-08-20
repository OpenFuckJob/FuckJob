//! 投递循环里的拟人化节奏执行器。
//!
//! 和 [`super::humanize`] 分开放，是因为那边刻意保持纯判断——人格推导与参数塑形
//! 全靠单测兜着，掺进睡眠和日志就没法跑。这边则相反，每个方法都在等时钟或写日志，
//! 但它们同样是平台无关的：BOSS 和猎聘的投递循环必须共用同一套节奏，否则两个
//! 平台会各自演化出一套「像人」的定义，而用户在配置页只调了一个开关。

use std::time::Duration;

use crate::config::HumanizeConfig;
use crate::logger;
use crate::rpa::humanize::{roll, scoped_persona, Persona, PersonaGuard};
use crate::rpa::run_flow::is_job_task_stop_requested;

/// 一轮投递期间的节奏控制。
///
/// 拟人化关闭时 `persona` 为 None，所有方法退化成改造前的行为：固定 3-5 秒停顿、
/// 不休息、不跳过。这条退路必须一直留着——功能默认关闭，绝大多数用户跑的是它
pub struct GreetPacer {
    persona: Option<Persona>,
    /// 距上次休息已经投出去多少条
    greets_since_break: u32,
    /// 本轮一共歇了几次，只用于日志
    breaks_taken: u32,
    /// 本轮跳过了几个岗位，只用于日志
    skimmed: u32,
    /// 把人格装进线程上下文，鼠标与打字动作在调用链深处据此决定要不要拟人。
    /// 节奏器一销毁，人格随之摘下——本轮的一切拟人行为都以它为界
    _persona_guard: PersonaGuard,
}

impl GreetPacer {
    /// `base_greets` 传用户设的单轮上限，休息节奏由它派生
    pub fn new(config: &HumanizeConfig, base_greets: u32) -> Self {
        let persona = Persona::today(config, base_greets);
        if let Some(persona) = persona.as_ref() {
            let _ = logger::info(format!("拟人化已启用，今日节奏：{}", persona.describe()));
        }
        Self {
            persona,
            greets_since_break: 0,
            breaks_taken: 0,
            skimmed: 0,
            _persona_guard: scoped_persona(persona),
        }
    }

    /// 拟人化没启用时的空档器，供不需要节奏控制的调用方使用
    pub fn disabled() -> Self {
        Self {
            persona: None,
            greets_since_break: 0,
            breaks_taken: 0,
            skimmed: 0,
            _persona_guard: scoped_persona(None),
        }
    }

    pub fn persona(&self) -> Option<&Persona> {
        self.persona.as_ref()
    }

    /// 这个岗位要不要「只看不投」。
    ///
    /// 跳过的岗位不入库，下一轮还会再遇到它——这是「今天先不投」而不是「永远不投」
    pub fn should_skim(&mut self) -> bool {
        let Some(persona) = self.persona.as_ref() else {
            return false;
        };
        if !persona.should_skim(roll()) {
            return false;
        }
        self.skimmed += 1;
        true
    }

    /// 处理完一个岗位之后的停顿，必要时转成一次完整的休息。
    ///
    /// `greeted` 表示招呼是否真的发出去了。没发出去（被闸门拦下、发送失败）
    /// 同样要停顿——那一段浏览、点击、等待页面的动作是实实在在发生过的——
    /// 但不该计进休息节奏：休息的依据是「连着投了多少条」，不是「连着看了多少个」。
    ///
    /// 返回 false 表示等待期间收到了停止请求，调用方应当立刻结束本轮
    pub async fn after_greet(&mut self, greeted: bool) -> bool {
        let pause = self.plan_pause(greeted);
        if let Pause::Break(millis) = pause {
            let seconds = millis / 1_000;
            let _ = logger::info(format!(
                "已连续投递 {} 条，休息 {} 分 {} 秒后继续",
                self.persona.map(|p| p.break_after_greets).unwrap_or_default(),
                seconds / 60,
                seconds % 60,
            ));
        }
        sleep_ms_interruptible(pause.millis()).await
    }

    /// 算出这一步之后该等多久，并推进休息计数。
    ///
    /// 与真正的等待分开：等待动辄十几分钟，混在一起就没法在单测里验证
    /// 「什么时候该歇」——而那恰恰是最容易算错、又最难在集成测试里看出来的部分
    fn plan_pause(&mut self, greeted: bool) -> Pause {
        let Some(persona) = self.persona else {
            // 改造前就是这么等的，关掉拟人化必须原样退回这里
            return Pause::Gap(pick_legacy_gap_ms());
        };

        if greeted {
            self.greets_since_break += 1;
            if persona.should_break(self.greets_since_break) {
                self.greets_since_break = 0;
                self.breaks_taken += 1;
                return Pause::Break(persona.break_seconds(roll()).saturating_mul(1_000));
            }
        }

        Pause::Gap(persona.gap_after_greet_ms(roll(), roll(), roll()))
    }

    /// 本轮的拟人化行为摘要，没有任何行为时返回 None
    pub fn summary(&self) -> Option<String> {
        if self.persona.is_none() || (self.breaks_taken == 0 && self.skimmed == 0) {
            return None;
        }
        Some(format!(
            "拟人化本轮休息 {} 次，随机跳过 {} 个岗位",
            self.breaks_taken, self.skimmed
        ))
    }
}

/// 一步之后要等多久，以及这段等待是普通停顿还是一次完整的休息
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pause {
    Gap(u64),
    Break(u64),
}

impl Pause {
    fn millis(self) -> u64 {
        match self {
            Self::Gap(millis) | Self::Break(millis) => millis,
        }
    }
}

/// 改造前投递循环里那段固定的 3-5 秒停顿
fn pick_legacy_gap_ms() -> u64 {
    3_000 + (roll() * 2_000.0).round() as u64
}

/// 可被停止请求打断的睡眠。返回 false 表示中途收到停止请求。
///
/// 按秒切片而不是一觉睡到底：休息可能长达二十分钟，用户点了停止不该干等着。
/// 不足一秒的尾巴直接睡掉，它不值得多绕一次判断
async fn sleep_ms_interruptible(millis: u64) -> bool {
    let whole_seconds = millis / 1_000;
    for _ in 0..whole_seconds {
        if is_job_task_stop_requested() {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if is_job_task_stop_requested() {
        return false;
    }
    let remainder = millis % 1_000;
    if remainder > 0 {
        tokio::time::sleep(Duration::from_millis(remainder)).await;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HumanizeIntensity;

    fn enabled_config() -> HumanizeConfig {
        HumanizeConfig {
            enabled: true,
            intensity: HumanizeIntensity::Standard,
            persona_seed: 0x1234_5678_9ABC_DEF0,
        }
    }

    /// 功能默认关闭，绝大多数用户跑的是这条路径：一个人格都不该被推出来
    #[test]
    fn a_disabled_config_produces_a_pacer_without_a_persona() {
        let pacer = GreetPacer::new(&HumanizeConfig::default(), 30);

        assert!(pacer.persona().is_none());
        assert!(pacer.summary().is_none());
    }

    #[test]
    fn an_enabled_config_produces_a_pacer_with_a_persona() {
        let pacer = GreetPacer::new(&enabled_config(), 30);

        assert!(pacer.persona().is_some());
    }

    /// 没有人格就一个岗位都不跳，拟人化关着时产出不该有任何损失
    #[test]
    fn a_pacer_without_a_persona_never_skims() {
        let mut pacer = GreetPacer::disabled();

        for _ in 0..50 {
            assert!(!pacer.should_skim());
        }
    }

    /// 没发生过休息或跳过时不出摘要，免得每轮日志尾巴都挂一句「休息 0 次」
    #[test]
    fn a_pacer_with_nothing_to_report_stays_silent() {
        let pacer = GreetPacer::new(&enabled_config(), 30);

        assert!(pacer.summary().is_none());
    }

    /// 休息的依据是「连着投了多少条」。把没投成的也算进去，休息会来得偏早，
    /// 极端情况下（连续发送失败）变成投 0 条歇一次
    #[test]
    fn only_delivered_greetings_count_towards_the_next_break() {
        let mut pacer = GreetPacer::new(&enabled_config(), 30);
        let threshold = pacer.persona().unwrap().break_after_greets;

        for _ in 0..threshold * 2 {
            assert!(matches!(pacer.plan_pause(false), Pause::Gap(_)));
        }

        assert_eq!(pacer.greets_since_break, 0);
        assert_eq!(pacer.breaks_taken, 0);
    }

    /// 没投成也得停一下：那一段浏览、点击、等页面的动作是真发生过的，
    /// 不停顿反而比正常投递还快
    #[test]
    fn a_failed_greeting_still_pauses() {
        let mut pacer = GreetPacer::new(&enabled_config(), 30);

        assert!(pacer.plan_pause(false).millis() > 0);
    }

    /// 投满阈值就歇，歇完计数归零，下一段重新数起
    #[test]
    fn a_break_arrives_on_the_threshold_and_resets_the_counter() {
        let mut pacer = GreetPacer::new(&enabled_config(), 30);
        let threshold = pacer.persona().unwrap().break_after_greets;

        for _ in 0..threshold - 1 {
            assert!(matches!(pacer.plan_pause(true), Pause::Gap(_)));
        }
        let pause = pacer.plan_pause(true);

        assert!(matches!(pause, Pause::Break(_)), "{pause:?}");
        // 群里提的「投 30 条歇 5-10 分钟」正是这个量级
        assert!(pause.millis() >= 240_000, "休息太短：{}ms", pause.millis());
        assert_eq!(pacer.greets_since_break, 0);
        assert_eq!(pacer.breaks_taken, 1);
        assert!(matches!(pacer.plan_pause(true), Pause::Gap(_)));
    }

    /// 拟人化关着时永远不休息，停顿也还是改造前那段 3-5 秒
    #[test]
    fn a_pacer_without_a_persona_never_breaks() {
        let mut pacer = GreetPacer::disabled();

        for _ in 0..200 {
            let pause = pacer.plan_pause(true);
            assert!(matches!(pause, Pause::Gap(_)));
            assert!((3_000..=5_000).contains(&pause.millis()));
        }
        assert_eq!(pacer.breaks_taken, 0);
    }

    #[test]
    fn the_summary_counts_breaks_and_skims() {
        let mut pacer = GreetPacer::new(&enabled_config(), 30);
        pacer.breaks_taken = 2;
        pacer.skimmed = 3;

        let summary = pacer.summary().unwrap();

        assert!(summary.contains("休息 2 次"));
        assert!(summary.contains("跳过 3 个岗位"));
    }

    /// 关掉拟人化时的停顿必须还是改造前那段 3-5 秒
    #[test]
    fn the_legacy_gap_stays_between_three_and_five_seconds() {
        for _ in 0..32 {
            let gap = pick_legacy_gap_ms();
            assert!((3_000..=5_000).contains(&gap), "{gap}");
        }
    }
}
