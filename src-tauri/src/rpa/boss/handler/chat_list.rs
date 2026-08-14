//! BOSS 沟通页左侧会话列表的分类切换与状态判定。
//!
//! 这里的每个选择器和每个时间常数都是连 CDP 到真实页面量出来的，不是照着渲染结果猜的。
//! 实测切到「未读」分类后的完整时序（页面冷加载，当时有 2 条未读）：
//!
//! ```text
//! +0.0s  点击「未读」
//! +1.6s  li.selected 变成「未读」      cards=0  no-data=0   ← 切换生效，数据还没回来
//! +2.5s  li.selected 仍是「未读」      cards=0  no-data=1   ← 「30天内暂无联系人」假空态
//! +3.6s  li.selected 变成「未读(2)」   cards=2  no-data=0   ← 真实数据到达，空态被替换
//! ```
//!
//! 两个坑都在这段时序里：
//!
//! 1. `.user-list-content` **不是常驻容器**——分类没有会话时它整个被卸载，
//!    `.user-list` 里换成 `.no-data`。把它当常驻容器 `wait` 在空分类下必然超时。
//! 2. `.no-data` **也不是权威空态**——它在等接口返回的一秒多里同样会出现。
//!    看见它就断言「没有未读」，正好撞进这个假空态窗口。
//!
//! 所以卡片可以见到就认（它不会凭空出现），空态必须连续稳定一段时间才认。

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use rust_drission::{utils::sleep_random_ms, Element, Page};

/// 分类切换完成后，会话列表的状态。
pub(crate) enum ListState {
    /// 列表已就位，至少一张会话卡片。
    Cards(Vec<Element>),
    /// 空态已经稳定住，这个分类确实没有会话。
    Empty,
    /// 等到超时也没等到卡片或稳定空态。既不能当成有，更不能当成没有。
    Unknown,
}

const LABEL_ITEM: &str = ".label-list li .label-name";
const SELECTED_LABEL: &str = ".label-list li.selected .label-name";
const CARD: &str = ".user-list-content .friend-content";
const EMPTY: &str = ".user-list .no-data";
/// 卡片上姓名+公司+职位那一块，见 [`conversation_key`]。
const CARD_TITLE: &str = ".title-box";
/// 借道刷新时经过的分类。「全部」一定存在，也一定是数据最全的那个。
const RELOAD_VIA: &str = "全部";

/// 空态要连续保持这么久才作数。实测假空态约 1.1 秒，留够两倍余量；
/// 代价只是真的没有未读时多等 3 秒，比误报「没有未读」便宜得多。
const EMPTY_MUST_HOLD: Duration = Duration::from_secs(3);

/// 等沟通页就绪。分类标签是页面骨架里最先稳定下来的部分，两个任务打开页面后
/// 的第一步都是等它——20 秒不是保守：一次冷加载实测就要 8 秒上下。
pub(crate) fn wait_for_chat_page(page: &Page, timeout: Duration) -> Result<(), anyhow::Error> {
    page.wait(LABEL_ITEM, timeout)
        .context("等待会话分类标签超时，页面可能未登录或未加载完成")?;
    Ok(())
}

/// 切换会话分类，并等到列表真正就位。
///
/// 不靠 sleep 猜时间：先确认 `li.selected` 已经是目标分类，再按模块顶部那段
/// 实测时序去等——卡片出现即认，空态则要连续稳定 [`EMPTY_MUST_HOLD`] 才认。
///
/// 找不到这个分类返回 `Err`：静默留在原分类的后果是拿别的分类的会话当成
/// 目标分类处理，比直接失败严重得多。
pub(crate) fn switch_label_and_wait(
    page: &Page,
    label: &str,
    timeout: Duration,
) -> Result<ListState, anyhow::Error> {
    let deadline = Instant::now() + timeout;

    if !is_selected(page, label)? {
        let mut target = None;
        for ele in page.eles(LABEL_ITEM)? {
            if ele.text_content()?.trim().starts_with(label) {
                target = Some(ele);
                break;
            }
        }
        let target =
            target.ok_or_else(|| anyhow!("沟通页没有「{label}」分类，页面结构可能已变"))?;
        target.click()?;

        // 点击到 selected 换人之间有几百毫秒，等不到就别往下走：
        // 后面读到的会是上一个分类的列表
        while !is_selected(page, label)? {
            if Instant::now() >= deadline {
                return Err(anyhow!("点击「{label}」分类后未切换过去"));
            }
            sleep_random_ms(150, 250);
        }
    }

    let mut empty_since: Option<Instant> = None;
    loop {
        let cards = page.elements(CARD)?;
        if !cards.is_empty() {
            return Ok(ListState::Cards(cards));
        }

        if page.ele(EMPTY)?.is_some() {
            let since = *empty_since.get_or_insert_with(Instant::now);
            // 标签上挂着计数就是铁证：数据已经回来了，且这个分类有会话。
            // 此时的空态只可能是渲染残留，无论它稳定多久都不能认
            if since.elapsed() >= EMPTY_MUST_HOLD && selected_count(page)?.unwrap_or(0) == 0 {
                return Ok(ListState::Empty);
            }
        } else {
            // 空态被真实列表顶掉了，稳定计时重来
            empty_since = None;
        }

        if Instant::now() >= deadline {
            // 等了这么久还是空态，那就是真空。但标签仍挂着计数的话说明页面
            // 自己都认为有会话，只是没渲染出来——那是异常，不能报成「没有」
            let empty_held = empty_since.is_some() && selected_count(page)?.unwrap_or(0) == 0;
            return Ok(if empty_held {
                ListState::Empty
            } else {
                ListState::Unknown
            });
        }
        sleep_random_ms(300, 450);
    }
}

/// 当前选中分类标签上的会话计数，如「未读(2)」里的 2、「未读(137)」里的 137。
///
/// 这是唯一能拿到**真实总数**的地方：列表是分页加载的，`.friend-content`
/// 只数得到已经渲染出来的那一页。
///
/// 实测这个计数和列表数据同时到达：数据没回来时标签只是光秃秃的「未读」。
/// 所以它只能用来否决空态和报数，不能拿来预判有没有未读。
pub(crate) fn selected_count(page: &Page) -> Result<Option<usize>, anyhow::Error> {
    let Some(selected) = page.ele(SELECTED_LABEL)? else {
        return Ok(None);
    };
    Ok(parse_label_count(&selected.text_content()?))
}

/// 从「未读(2)」这类标签文本里取计数；没有数字返回 None。
fn parse_label_count(text: &str) -> Option<usize> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 重新拉取某个分类的列表。
///
/// 已经停在这个分类上时，[`switch_label_and_wait`] 不会再点一次，读到的就还是
/// 上一轮那份 DOM——处理过的会话仍挂在里面，看着像永远处理不完。所以先借道
/// 「全部」再切回来，逼页面重新请求一次。处理完一个会话就得这样刷一次，
/// 否则拿到的永远是同一批卡片。
pub(crate) fn reload_label(
    page: &Page,
    label: &str,
    timeout: Duration,
) -> Result<ListState, anyhow::Error> {
    if is_selected(page, label)? {
        switch_label_and_wait(page, RELOAD_VIA, timeout)?;
    }
    switch_label_and_wait(page, label, timeout)
}

/// 当前选中的分类是不是它。
fn is_selected(page: &Page, label: &str) -> Result<bool, anyhow::Error> {
    let Some(selected) = page.ele(SELECTED_LABEL)? else {
        return Ok(false);
    };
    Ok(selected.text_content()?.trim().starts_with(label))
}

/// 会话卡片的去重标识。
///
/// 卡片上没有可用的 id（`d-c` 实测是埋点常量，40 张卡片全是 62001），只能用文本。
/// 但**不能用整张卡片的文本**：里面混着未读徽标的数字、消息时间和最后一条消息预览，
/// 点开会话徽标一消失文本就变了，同一个会话会被当成新会话再处理一遍。
/// `.title-box` 里只有姓名、公司、职位，不随已读状态变化。
pub(crate) fn conversation_key(card: &Element) -> Result<String, anyhow::Error> {
    let text = match card.element(CARD_TITLE)? {
        Some(title) => title.text_content()?,
        // 页面结构变了也别让整轮任务崩掉，退回整卡片文本还能凑合去重
        None => card.text_content()?,
    };
    Ok(normalize_key(&text))
}

fn normalize_key(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_count_from_selected_label() {
        assert_eq!(parse_label_count("未读(2)"), Some(2));
        assert_eq!(parse_label_count("未读 (12)"), Some(12));
        // 数据还没回来时标签就是光秃秃的「未读」，这时候什么都不知道
        assert_eq!(parse_label_count("未读"), None);
        assert_eq!(parse_label_count("全部"), None);
    }

    #[test]
    fn key_normalizes_whitespace() {
        assert_eq!(
            normalize_key("  王瑞  爱视文化\n\n总经理  "),
            "王瑞 爱视文化 总经理"
        );
        assert_eq!(normalize_key("   "), "");
    }
}
