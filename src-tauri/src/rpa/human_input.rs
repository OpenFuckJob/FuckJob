//! 拟人化的输入动作：鼠标轨迹、逐字符打字、分段滚动。
//!
//! 这一层的收益和风险都很直接。收益是 `element.click()` 走的是 JS 派发，
//! `isTrusted` 为 false，而这里发的是 CDP `Input` 事件，浏览器眼里就是真实输入；
//! 风险是真实点击会打在坐标上——元素被遮挡、不在视口、rect 拿不到，点出去就是
//! 一次误触。
//!
//! 所以每个函数都遵守同一条纪律：**能拟人则拟人，拿不准就原样回退**。
//! 拟人化是为了少被风控盯上，不是为了把投递本身搞挂——一次误点的代价（给错误的
//! BOSS 发招呼、点进无关页面）远大于一次 `isTrusted: false`。

use std::time::Duration;

use anyhow::anyhow;
use rust_drission::{Element, Page};
use serde_json::{json, Value};

use crate::rpa::humanize::{current_persona, roll, Persona};
use crate::rpa::run_flow::is_job_task_stop_requested;

/// 元素小于这个尺寸就不做坐标点击了。
///
/// 几像素的目标上，轨迹抖动很容易把落点甩到边界外，得不偿失
const MIN_CLICKABLE_SIZE: f64 = 8.0;

/// 一次点击里按下与抬起之间的停顿区间（毫秒）。真人按不出 0 毫秒的键程
const CLICK_HOLD_MS: (u64, u64) = (45, 130);

/// 元素在视口里的位置与尺寸
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 视口尺寸
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// 这个矩形值不值得走坐标点击。
    ///
    /// 完全在视口之外、或者小得抖一下就脱靶的目标一律交回 JS 点击
    pub fn is_clickable_within(&self, viewport: Viewport) -> bool {
        self.width >= MIN_CLICKABLE_SIZE
            && self.height >= MIN_CLICKABLE_SIZE
            && self.x >= 0.0
            && self.y >= 0.0
            && self.x + self.width <= viewport.width
            && self.y + self.height <= viewport.height
    }

    /// 落点：中心附近的一个随机位置，但不贴边。
    ///
    /// 每次都精确点在几何中心是比 `isTrusted: false` 更刺眼的特征——
    /// 真人的落点是一片云，不是一个点。`roll_x` / `roll_y` 取 0..=1
    pub fn click_point(&self, roll_x: f64, roll_y: f64) -> (f64, f64) {
        // 只在中间 60% 的范围里取点，留出边距扛住轨迹抖动
        let spread_x = self.width * 0.3;
        let spread_y = self.height * 0.3;
        let center_x = self.x + self.width / 2.0;
        let center_y = self.y + self.height / 2.0;
        (
            center_x + (roll_x.clamp(0.0, 1.0) * 2.0 - 1.0) * spread_x,
            center_y + (roll_y.clamp(0.0, 1.0) * 2.0 - 1.0) * spread_y,
        )
    }
}

/// 从落点倒推一条进场轨迹。
///
/// 真人的鼠标不会瞬移，也不会走直线。这里用一条二次贝塞尔：起点在目标外侧
/// 一段距离，控制点偏到一边制造弧度，落点即目标。`steps` 越大越细腻，
/// 代价是每一步一次 CDP 往返
pub fn approach_path(target: (f64, f64), steps: u32, rolls: (f64, f64, f64)) -> Vec<(f64, f64)> {
    let steps = steps.clamp(2, 40);
    let (roll_angle, roll_distance, roll_bend) = rolls;

    // 起点落在目标周围 120-360 像素的一个方向上
    let angle = roll_angle.clamp(0.0, 1.0) * std::f64::consts::TAU;
    let distance = 120.0 + roll_distance.clamp(0.0, 1.0) * 240.0;
    let start = (
        target.0 + angle.cos() * distance,
        target.1 + angle.sin() * distance,
    );

    // 控制点垂直于起点-终点连线偏出去，弧度左右不定
    let bend = (roll_bend.clamp(0.0, 1.0) * 2.0 - 1.0) * distance * 0.4;
    let mid = ((start.0 + target.0) / 2.0, (start.1 + target.1) / 2.0);
    let direction = (target.0 - start.0, target.1 - start.1);
    let length = (direction.0 * direction.0 + direction.1 * direction.1).sqrt().max(1.0);
    let normal = (-direction.1 / length, direction.0 / length);
    let control = (mid.0 + normal.0 * bend, mid.1 + normal.1 * bend);

    (1..=steps)
        .map(|step| {
            // 末段放慢：ease-out 让轨迹在接近目标时变密，和真人收手的样子一致
            let linear = step as f64 / steps as f64;
            let t = 1.0 - (1.0 - linear).powi(2);
            let inverse = 1.0 - t;
            (
                inverse * inverse * start.0 + 2.0 * inverse * t * control.0 + t * t * target.0,
                inverse * inverse * start.1 + 2.0 * inverse * t * control.1 + t * t * target.1,
            )
        })
        .collect()
}

/// 点击一个元素。
///
/// 装了人格就走真实鼠标事件，没装、或者坐标条件不满足就回退到 JS 点击。
/// 无论走哪条路，「点击这个元素」这件事一定会发生——回退不是失败
pub fn click(page: &Page, element: &Element) -> Result<(), anyhow::Error> {
    let Some(persona) = current_persona() else {
        return Ok(element.click()?);
    };
    match trusted_click(page, element, &persona) {
        Ok(()) => Ok(()),
        Err(_) => {
            // 拿不到坐标、元素被遮挡、CDP 拒绝——都不值得让整条投递流程失败
            element.click()?;
            Ok(())
        }
    }
}

/// 真实鼠标点击。任何一步不满足条件都返回 Err，交给调用方回退
fn trusted_click(page: &Page, element: &Element, persona: &Persona) -> Result<(), anyhow::Error> {
    let viewport = read_viewport(page)?;
    let rect = read_rect(element)?;
    if !rect.is_clickable_within(viewport) {
        return Err(anyhow!("元素不在可安全点击的范围内"));
    }

    let target = rect.click_point(roll(), roll());
    let steps = persona.mouse_steps(roll());
    for point in approach_path(target, steps, (roll(), roll(), roll())) {
        page.dispatch_mouse_event("mouseMoved", point.0, point.1, None, None)?;
        // 每步之间的微停顿，凑出一条有速度变化的轨迹
        sleep_ms(2 + (roll() * 12.0) as u64);
    }

    // 停在目标上短暂悬停再按下：真人的手要先停稳
    sleep_ms(40 + (roll() * 160.0) as u64);
    page.dispatch_mouse_event("mousePressed", target.0, target.1, Some("left"), Some(1))?;
    sleep_ms(CLICK_HOLD_MS.0 + (roll() * (CLICK_HOLD_MS.1 - CLICK_HOLD_MS.0) as f64) as u64);
    page.dispatch_mouse_event("mouseReleased", target.0, target.1, Some("left"), Some(1))?;
    Ok(())
}

/// 逐字符敲进目标元素。返回 false 表示这条路走不通，调用方应当回退到原有写法。
///
/// 只负责把字打进去，不负责发送——发送按钮的可用状态由站点自己的输入事件驱动，
/// 而 CDP `Input.insertText` 触发的正是真实的 `beforeinput` / `input`
pub fn type_text(page: &Page, element: &Element, text: &str) -> Result<bool, anyhow::Error> {
    let Some(persona) = current_persona() else {
        return Ok(false);
    };
    if text.is_empty() {
        return Ok(false);
    }
    if element.focus().is_err() {
        return Ok(false);
    }
    sleep_ms(120 + (roll() * 380.0) as u64);

    for (index, character) in text.chars().enumerate() {
        if is_job_task_stop_requested() {
            // 半截消息比不发更糟，但这里已经打进去的内容不会被发送——
            // 发送动作在调用方，且停止请求同样会拦住它
            return Ok(false);
        }
        if page
            .run_cdp(
                "Input.insertText",
                Some(json!({ "text": character.to_string() })),
            )
            .is_err()
        {
            // 第一个字符就失败说明这条路不通，交给调用方原样回退；
            // 打到一半才失败则已经产生了残缺内容，必须让调用方清干净重来
            return Ok(false);
        }
        sleep_ms(keystroke_delay_ms(&persona, character, index, roll(), roll()));
    }

    sleep_ms(persona.review_before_send_ms(roll()));
    Ok(true)
}

/// 敲下一个字符之后停多久。
///
/// 标点后面停得久一点——真人在这里换气、想下一句怎么写；开头几个字也慢，
/// 手还没热起来
pub fn keystroke_delay_ms(
    persona: &Persona,
    character: char,
    index: usize,
    speed_roll: f64,
    pause_roll: f64,
) -> u64 {
    let mut delay = persona.typing_delay_ms(speed_roll);
    if index < 3 {
        delay = (delay as f64 * 1.4).round() as u64;
    }
    if matches!(
        character,
        '，' | '。' | '！' | '？' | '；' | '：' | ',' | '.' | '!' | '?' | ';' | ':' | '\n'
    ) {
        // 三成的句读会真的停顿，全停就成了另一种规律
        if pause_roll.clamp(0.0, 1.0) < 0.3 {
            delay = delay.saturating_add(240 + (pause_roll * 2_000.0) as u64);
        }
    }
    delay
}

/// 分几段滚到底，而不是一脚踩到 `scrollHeight`。
///
/// 瞬移到底部这个动作真人做不出来——滚轮一次转不了半个页面，触控板也不行。
/// 返回滚动结束后的页面高度；没装人格时返回 None，调用方走原有的滚动路径
pub fn scroll_to_bottom(page: &Page) -> Result<Option<f64>, anyhow::Error> {
    let Some(persona) = current_persona() else {
        return Ok(None);
    };
    let steps = 3 + (roll() * 4.0) as u32;
    for _ in 0..steps {
        if is_job_task_stop_requested() {
            break;
        }
        // 一次滚一屏上下，带随机余量
        let delta = 320.0 + roll() * 520.0;
        page.run_js_await(&format!(
            r#"
(() => {{
    const html = document.documentElement;
    const body = document.body;
    const container = html.scrollHeight > html.clientHeight ? html : body;
    container.scrollTop = container.scrollTop + {delta};
    window.dispatchEvent(new Event('scroll'));
    return container.scrollTop;
}})();
"#
        ))?;
        // 先按浮点算完再取整：pace 是 1.2 这种小数，先转 u64 会被截成 1，缩放就没了
        sleep_ms(((180.0 + roll() * 420.0) * persona.pace.clamp(0.5, 3.0)).round() as u64);
    }

    // 最后一段补到底，保证懒加载观察器一定被唤醒
    let settled = page.run_js_await(
        r#"
(() => {
    const html = document.documentElement;
    const body = document.body;
    const container = html.scrollHeight > html.clientHeight ? html : body;
    container.scrollTop = container.scrollHeight;
    window.dispatchEvent(new Event('scroll'));
    return { scrollHeight: container.scrollHeight, scrollTop: container.scrollTop };
})();
"#,
    )?;
    Ok(Some(scroll_height_of(&settled).unwrap_or_default()))
}

fn read_viewport(page: &Page) -> Result<Viewport, anyhow::Error> {
    let value = page.run_js_await("({ width: window.innerWidth, height: window.innerHeight })")?;
    let raw = value.get("value").unwrap_or(&value);
    let width = raw.get("width").and_then(Value::as_f64).unwrap_or(0.0);
    let height = raw.get("height").and_then(Value::as_f64).unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 {
        return Err(anyhow!("读不到视口尺寸"));
    }
    Ok(Viewport { width, height })
}

fn read_rect(element: &Element) -> Result<Rect, anyhow::Error> {
    let value = element.rect()?;
    let raw = value.get("value").unwrap_or(&value);
    Ok(Rect {
        x: raw.get("x").and_then(Value::as_f64).ok_or_else(|| anyhow!("元素缺少 x 坐标"))?,
        y: raw.get("y").and_then(Value::as_f64).ok_or_else(|| anyhow!("元素缺少 y 坐标"))?,
        width: raw
            .get("width")
            .and_then(Value::as_f64)
            .ok_or_else(|| anyhow!("元素缺少宽度"))?,
        height: raw
            .get("height")
            .and_then(Value::as_f64)
            .ok_or_else(|| anyhow!("元素缺少高度"))?,
    })
}

/// 从 JS 返回值里取 scrollHeight，兼容 `{value: {...}}` 包装
fn scroll_height_of(value: &Value) -> Option<f64> {
    let raw = value.get("value").unwrap_or(value);
    raw.get("scrollHeight").and_then(Value::as_f64)
}

/// 输入动作全程跑在 rust_drission 的同步 API 上，这里跟着用同步睡眠
fn sleep_ms(millis: u64) {
    if millis > 0 {
        std::thread::sleep(Duration::from_millis(millis));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HumanizeConfig, HumanizeIntensity};
    use chrono::NaiveDate;

    fn persona() -> Persona {
        Persona::derive(
            &HumanizeConfig {
                enabled: true,
                intensity: HumanizeIntensity::Standard,
                persona_seed: 0x0BAD_C0FF_EE00_1234,
            },
            NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
            30,
        )
        .unwrap()
    }

    fn viewport() -> Viewport {
        Viewport {
            width: 1440.0,
            height: 900.0,
        }
    }

    #[test]
    fn a_normal_button_inside_the_viewport_is_clickable() {
        let rect = Rect {
            x: 400.0,
            y: 300.0,
            width: 120.0,
            height: 36.0,
        };

        assert!(rect.is_clickable_within(viewport()));
    }

    /// 滚出视口的元素坐标是负的，照着点会打在别的东西上
    #[test]
    fn an_element_scrolled_out_of_view_is_not_clickable() {
        let above = Rect {
            x: 400.0,
            y: -60.0,
            width: 120.0,
            height: 36.0,
        };
        let below = Rect {
            x: 400.0,
            y: 880.0,
            width: 120.0,
            height: 36.0,
        };

        assert!(!above.is_clickable_within(viewport()));
        assert!(!below.is_clickable_within(viewport()));
    }

    /// 几像素的目标上轨迹抖动会把落点甩出去，不如交回 JS 点击
    #[test]
    fn a_tiny_element_is_not_worth_a_coordinate_click() {
        let rect = Rect {
            x: 400.0,
            y: 300.0,
            width: 4.0,
            height: 4.0,
        };

        assert!(!rect.is_clickable_within(viewport()));
    }

    /// 落点必须散开，每次都精确命中几何中心比 isTrusted 更刺眼
    #[test]
    fn click_points_scatter_around_the_center() {
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 50.0,
        };

        let center = rect.click_point(0.5, 0.5);
        let corner = rect.click_point(0.0, 1.0);

        assert_eq!(center, (200.0, 125.0));
        assert_ne!(corner, center);
    }

    /// 散开归散开，落点绝不能跑到元素外面去
    #[test]
    fn click_points_never_leave_the_element() {
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 50.0,
        };

        for roll_x in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for roll_y in [0.0, 0.5, 1.0] {
                let (x, y) = rect.click_point(roll_x, roll_y);
                assert!(x >= rect.x && x <= rect.x + rect.width, "x={x}");
                assert!(y >= rect.y && y <= rect.y + rect.height, "y={y}");
            }
        }
    }

    /// 越界的随机源不该把落点甩出元素
    #[test]
    fn out_of_range_rolls_keep_the_click_point_inside() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let (x, y) = rect.click_point(-9.0, 42.0);

        assert!((0.0..=100.0).contains(&x));
        assert!((0.0..=100.0).contains(&y));
    }

    /// 轨迹的终点必须正好落在目标上，否则点击就偏了
    #[test]
    fn the_approach_path_ends_exactly_on_the_target() {
        let path = approach_path((640.0, 480.0), 10, (0.3, 0.7, 0.2));

        let last = *path.last().unwrap();
        assert!((last.0 - 640.0).abs() < 0.001, "{last:?}");
        assert!((last.1 - 480.0).abs() < 0.001, "{last:?}");
        assert_eq!(path.len(), 10);
    }

    /// 直线轨迹本身就是特征，路径中段必须偏离起终点连线
    #[test]
    fn the_approach_path_curves_instead_of_running_straight() {
        let target = (640.0, 480.0);
        let path = approach_path(target, 12, (0.0, 0.5, 1.0));
        let start = path[0];

        let midpoint = path[path.len() / 2];
        let straight_x = start.0 + (target.0 - start.0) * 0.5;
        let straight_y = start.1 + (target.1 - start.1) * 0.5;
        let drift = ((midpoint.0 - straight_x).powi(2) + (midpoint.1 - straight_y).powi(2)).sqrt();

        assert!(drift > 1.0, "轨迹几乎是直线，drift={drift}");
    }

    /// 收手时步子变密，和真人接近目标时减速一致
    #[test]
    fn the_approach_path_decelerates_towards_the_target() {
        let path = approach_path((640.0, 480.0), 12, (0.25, 0.5, 0.0));

        let first_leg = distance(path[0], path[1]);
        let last_leg = distance(path[path.len() - 2], path[path.len() - 1]);

        assert!(last_leg < first_leg, "first={first_leg} last={last_leg}");
    }

    /// 步数被夹在合理区间，免得 CDP 往返把一次点击拖成几秒
    #[test]
    fn the_step_count_is_clamped_to_a_sane_range() {
        assert_eq!(approach_path((10.0, 10.0), 0, (0.5, 0.5, 0.5)).len(), 2);
        assert_eq!(approach_path((10.0, 10.0), 999, (0.5, 0.5, 0.5)).len(), 40);
    }

    /// 开头几个字慢一些，手还没热起来
    #[test]
    fn the_first_keystrokes_are_slower_than_the_rest() {
        let persona = persona();

        let opening = keystroke_delay_ms(&persona, '你', 0, 0.5, 0.9);
        let settled = keystroke_delay_ms(&persona, '你', 9, 0.5, 0.9);

        assert!(opening > settled);
    }

    /// 句读处会换气，但不能每个逗号都停——那是另一种规律
    #[test]
    fn punctuation_sometimes_pauses_and_sometimes_does_not() {
        let persona = persona();

        let paused = keystroke_delay_ms(&persona, '，', 9, 0.5, 0.0);
        let flowed = keystroke_delay_ms(&persona, '，', 9, 0.5, 0.9);

        assert!(paused > flowed);
        assert_eq!(flowed, keystroke_delay_ms(&persona, '字', 9, 0.5, 0.9));
    }

    /// 一条几十字的招呼语打完要花多久，量级上必须还是「人在打字」而不是「人在发呆」
    #[test]
    fn typing_a_greeting_takes_a_human_amount_of_time() {
        let persona = persona();
        let text = "您好，我看到贵司在招聘后端工程师，我有五年相关经验，方便聊聊吗？";

        let total: u64 = text
            .chars()
            .enumerate()
            .map(|(index, character)| keystroke_delay_ms(&persona, character, index, 0.5, 0.5))
            .sum();

        assert!(total > 4_000, "打得太快：{total}ms");
        assert!(total < 90_000, "打得太慢：{total}ms");
    }

    #[test]
    fn scroll_height_is_read_from_wrapped_and_plain_results() {
        assert_eq!(
            scroll_height_of(&json!({"value": {"scrollHeight": 4200}})),
            Some(4200.0)
        );
        assert_eq!(scroll_height_of(&json!({"scrollHeight": 4200.5})), Some(4200.5));
        assert_eq!(scroll_height_of(&json!({"foo": 1})), None);
    }

    fn distance(from: (f64, f64), to: (f64, f64)) -> f64 {
        ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt()
    }
}
