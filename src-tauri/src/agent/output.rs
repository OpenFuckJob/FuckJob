//! 大模型输出的净化与体检。
//!
//! 这里处理的文本会原样发进招聘平台聊天框、被真人 HR 读到，而不是喂给下一个程序。
//! 模型的思考过程、markdown 包装、没被替换掉的模板占位符、以及「作为一个 AI 我无法……」
//! 这类拒答话术，任何一样漏出去都是当场暴露账号在跑脚本。宁可保守一点少说一句，
//! 也不能把这些东西送到对面。
//!
//! 所以本模块只做两件事：把原始输出洗成可直接发送的正文（[`sanitize`]），
//! 以及在发送前给正文做几项体检（[`looks_like_refusal`]、[`has_placeholder`]）。
//! 判定一律偏保守：宁可漏判也不能误伤正文——正文被截坏同样会发给 HR。
//!
//! 模块刻意不引用项目内任何类型，只依赖标准库与 regex。净化规则要靠大量单测钉死行为，
//! 无依赖才能让每条规则都能被单独构造用例验证，也方便在调用链的任意位置复用。

use regex::Regex;
use std::sync::LazyLock;

/// 推理标签成对出现的情况。用非贪婪匹配，遇到「一次回答里吐了多段思考」时逐段剥离；
/// 开闭标签不做名称配对（regex crate 无反向引用），实际模型也不会吐出交叉嵌套的标签。
static REASONING_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<\s*(?:thinking|think|reasoning)\b[^>]*>.*?<\s*/\s*(?:thinking|think|reasoning)\s*>",
    )
    .expect("valid regex")
});

/// DeepSeek-R1 一类模型会只吐结束标签而没有开始标签，此时标签之前的全部内容都是思考。
static REASONING_ORPHAN_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)^.*?<\s*/\s*(?:thinking|think|reasoning)\s*>").expect("valid regex")
});

/// 生成被 max_tokens 截断时只剩开始标签。此时后面全是没写完的思考，整段丢掉比留着安全。
static REASONING_ORPHAN_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*(?:thinking|think|reasoning)\b[^>]*>.*$").expect("valid regex")
});

/// 代码围栏的起始行：连续反引号加可选语言标注，整行不能有别的内容。
static FENCE_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(`{3,})[ \t]*[A-Za-z0-9_+#.\-]*$").expect("valid regex"));

/// 「以下是……：」这类引导句独占一行的形态。冒号后必须换行，避免把正文里的冒号句吃掉。
static LEAD_IN_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[ \t]*(?:(?:好的|当然可以|当然|没问题|明白了|明白|收到|了解|ok|okay|sure|certainly)[，,、。.！!：:]*[ \t]*)?(?:以下|下面|这里是|这是|回复|答复|内容|正文|结果)[^：:\n]{0,40}[：:][ \t]*\r?\n",
    )
    .expect("valid regex")
});

/// 引导句和正文挤在同一行的形态。这里只认「以下/下面/这里是/这是」这种明确的announce开头，
/// 比独占一行的规则更容易误伤，所以入口词收得更紧。
static LEAD_IN_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[ \t]*(?:(?:好的|当然可以|当然|没问题|明白了|明白|收到|了解|ok|okay|sure|certainly)[，,、。.！!：:]*[ \t]*)?(?:以下|下面|这里是|这是)[^：:\n]{0,40}[：:][ \t]*",
    )
    .expect("valid regex")
});

/// 只有寒暄、没有下文的一整行（「好的，」单独占一行）。同一行还有正文时不动它，
/// 因为「好的，我下周一可以入职」里的「好的」是真话，不是壳。
static GREETING_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[ \t]*(?:好的|当然可以|当然|没问题|明白了|明白|收到|了解|ok|okay|sure|certainly)[，,、。.！!：:]+[ \t]*\r?\n",
    )
    .expect("valid regex")
});

/// 引导句必须自指「我生成的这段内容」才剥。缺了这道校验，
/// 「以下是我的项目经历：」这种正文结构也会被当成壳切掉。
const LEAD_IN_MARKERS: &[&str] = &[
    "回复",
    "答复",
    "内容",
    "消息",
    "文案",
    "正文",
    "打招呼",
    "招呼语",
    "话术",
    "结果",
    "如下",
    "为您",
    "给您",
    "为你",
    "生成",
    "撰写",
    "准备",
];

/// 收尾寒暄的起手词。必须同时命中 [`TRAILING_SHELL_MARKERS`] 才算壳。
const TRAILING_SHELL_STARTERS: &[&str] = &[
    "希望",
    "如需",
    "如果",
    "若需",
    "需要",
    "以上",
    "请随时",
    "注：",
    "注:",
    "温馨提示",
    "hope",
    "let me know",
    "feel free",
];

/// 收尾寒暄的判别词。刻意不收「告诉我」这种泛词：
/// 「如果您方便，可以告诉我面试时间。」是正文，不是壳。
const TRAILING_SHELL_MARKERS: &[&str] = &[
    "有帮助",
    "有所帮助",
    "帮到您",
    "帮到你",
    "仅供参考",
    "供您参考",
    "供参考",
    "如需调整",
    "如需修改",
    "如需补充",
    "需要调整",
    "需要修改",
    "需要优化",
    "随时告诉我",
    "是否满意",
    "不满意",
    "this helps",
    "let me know",
];

/// 连续空行折叠。模型爱在段落间灌三四个换行，发到聊天框里会撑成一屏。
static EXTRA_NEWLINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").expect("valid regex"));

/// 占位符：连续大写 X / N / 某 加上量词单位。单独一个 X 不算，
/// 否则「X光」「Xbox」这类正常词会被判成没填的模板。
static PLACEHOLDER_UNIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:[XＸ]+|N)(?:年|个月|月|岁|公司|企业|岗位|职位|部门|团队|项目|产品|学校|大学|元|万|人)|某某|某(?:公司|企业|岗位|职位|部门|项目|产品|学校|大学|城市|地区)",
    )
    .expect("valid regex")
});

/// 三个及以上连续的 X 本身就没有正常语义，不用再看后面跟了什么。
static PLACEHOLDER_REPEAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[XxＸｘ]{3,}").expect("valid regex"));

/// 方括号/书名号占位。括号内必须出现「待补充」这类槽位词才算，
/// 不然「[Java]」「[1]」这种正常写法会被误杀。
static PLACEHOLDER_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)[\[【][^\]】\n]{0,24}(?:待补充|待填写|待定|请填写|请输入|请补充|此处|占位|插入|替换|自行|公司名|岗位名|职位名|姓名|名称|你的|您的|your|todo|xx|\.\.\.|…)[^\]】\n]{0,24}[\]】]",
    )
    .expect("valid regex")
});

/// 模板变量没被渲染。项目内的提示词渲染用的就是 `{{ }}`，漏渲染会原样带到聊天框。
static PLACEHOLDER_TEMPLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{[^{}\n]{0,40}\}\}").expect("valid regex"));

/// 英文占位。加词边界，避免命中正常单词的一部分。
static PLACEHOLDER_ENGLISH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:todo|fixme|(?:your|user|company|job|position|candidate)_[a-z0-9_]+)\b")
        .expect("valid regex")
});

/// 拒答话术的检测窗口。「我无法」出现在第三段是正常语义（比如谈入职时间），
/// 出现在开头才是模型在推诿，所以只看开头这么多个字符。
const REFUSAL_HEAD_CHARS: usize = 60;

/// 这些说法在求职语境里不可能是正常表达，命中即判拒答。
const REFUSAL_STRONG: &[&str] = &[
    "作为一个ai",
    "作为一名ai",
    "作为ai",
    "作为人工智能",
    "作为一个人工智能",
    "作为一名人工智能",
    "作为语言模型",
    "作为一个语言模型",
    "作为大语言模型",
    "作为一个大语言模型",
    "as an ai",
    "as a language model",
    "as an artificial intelligence",
    "i apologize, but",
    "i'm sorry, but",
    "i am sorry, but",
];

/// 这些说法本身是中性的，必须紧跟着一个拒答宾语才算数——
/// 「我无法在下周入职」是正常答复，「我无法提供该内容」才是拒答。
const REFUSAL_WEAK_LEADS: &[&str] = &[
    "我无法",
    "我不能",
    "我不会",
    "很抱歉",
    "非常抱歉",
    "抱歉",
    "对不起",
    "i cannot",
    "i can't",
    "i can not",
    "i'm unable",
    "i am unable",
    "i won't",
    "i apologize",
];

/// 与 [`REFUSAL_WEAK_LEADS`] 搭配才成立的拒答宾语。
const REFUSAL_OBJECTS: &[&str] = &[
    "提供",
    "生成",
    "创作",
    "撰写",
    "编写",
    "完成",
    "协助",
    "帮助",
    "回答",
    "满足",
    "执行",
    "处理",
    "参与",
    "讨论",
    "继续",
    "该请求",
    "这个请求",
    "此类",
    "这类",
    "上述要求",
    "provide",
    "assist",
    "generate",
    "create",
    "complete",
    "fulfill",
    "comply",
    "help",
    "answer",
    "that request",
    "this request",
];

/// 弱拒答词与宾语之间允许隔多远。中文一个字信息量更大，窗口给得比英文紧，
/// 否则「我无法在下周入职，可以提供离职证明」会被误判成拒答。
const REFUSAL_GAP_CJK: usize = 8;
const REFUSAL_GAP_ASCII: usize = 16;

/// 把模型原始输出净化成可直接使用的正文。
pub fn sanitize(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let text = strip_reasoning(&text);
    let text = strip_code_fence(&text);
    // 壳剥完以后围栏可能才露出来（「好的，以下是内容：\n```json\n...\n```」),
    // 所以围栏要再走一遍。
    let text = strip_shell(&text);
    let text = strip_code_fence(&text);
    let text = EXTRA_NEWLINES.replace_all(&text, "\n\n");
    text.trim().to_string()
}

/// 从净化后的文本里抽出第一段完整 JSON 对象，抽不到返回 None。
pub fn extract_json(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|byte| *byte == b'{')?;

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    // 按字节扫描是安全的：UTF-8 多字节序列的后续字节最高位恒为 1，
    // 不可能与这里比较的 ASCII 字符相等，切片边界也就一定落在字符边界上。
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 文本是否像模型的拒答/免责话术（这类内容绝不能发给 HR）。
pub fn looks_like_refusal(text: &str) -> bool {
    let head = text
        .trim_start()
        .chars()
        .take(REFUSAL_HEAD_CHARS)
        .collect::<String>()
        .to_lowercase();
    if head.is_empty() {
        return false;
    }
    if REFUSAL_STRONG.iter().any(|marker| head.contains(marker)) {
        return true;
    }
    REFUSAL_WEAK_LEADS
        .iter()
        .any(|lead| weak_refusal_hit(&head, lead))
}

/// 文本是否残留未填充的占位符（发出去会暴露是机器人）。
pub fn has_placeholder(text: &str) -> bool {
    PLACEHOLDER_UNIT.is_match(text)
        || PLACEHOLDER_REPEAT.is_match(text)
        || PLACEHOLDER_BRACKET.is_match(text)
        || PLACEHOLDER_TEMPLATE.is_match(text)
        || PLACEHOLDER_ENGLISH.is_match(text)
}

/// 断句点太靠前时宁可硬截也不采用的比例下限。
///
/// 没有这条下限时，「一句话开头 + 一大段没断句的正文」会被砍到只剩开头那一句：
/// 上限 200 字、唯一句号在第 12 字，结果就只发出 12 个字。
/// 半截话发给 HR 比多一句没说完的话糟糕得多。
const MIN_SENTENCE_CUT_RATIO: f64 = 0.6;

/// 按「字符数」截断（不是字节数，中文必须正确处理），超长时优先断在句末标点。
pub fn truncate_at_sentence(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head = text.chars().take(max_chars).collect::<String>();
    let floor = (max_chars as f64 * MIN_SENTENCE_CUT_RATIO) as usize;

    match head.rfind(is_sentence_end) {
        Some(index) => {
            let punctuation_len = head[index..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or_default();
            let cut = &head[..index + punctuation_len];
            if cut.chars().count() < floor {
                // 断句点太靠前，保内容完整性优先
                return head.trim_end().to_string();
            }
            cut.trim_end().to_string()
        }
        // 整段没有句末标点（比如一长串没断句的自我介绍），只能硬截。
        None => head.trim_end().to_string(),
    }
}

fn is_sentence_end(value: char) -> bool {
    matches!(value, '。' | '！' | '？' | '!' | '?' | '.' | '\n')
}

fn strip_reasoning(text: &str) -> String {
    let text = REASONING_BLOCK.replace_all(text, "");
    // 先处理孤立结束标签再处理孤立开始标签：「</think>正文<think>没写完」两种残缺同时出现时，
    // 这个顺序才能把中间的正文留下来。
    let text = REASONING_ORPHAN_CLOSE.replace(&text, "");
    REASONING_ORPHAN_OPEN.replace(&text, "").into_owned()
}

fn strip_code_fence(text: &str) -> String {
    let mut current = text.trim().to_string();
    // 允许多层：外层用四个反引号包住内层三个反引号是模型常见的嵌套写法。
    for _ in 0..3 {
        match unwrap_fence_once(&current) {
            Some(next) => current = next,
            None => break,
        }
    }
    current
}

fn unwrap_fence_once(text: &str) -> Option<String> {
    let lines = text.lines().collect::<Vec<_>>();
    let captures = FENCE_OPEN.captures(lines.first()?.trim())?;
    let ticks = captures.get(1)?.as_str().len();

    // 按 CommonMark 的规矩，闭合围栏的反引号数量不少于开围栏，内层短围栏因此不会提前闭合。
    let closing = lines.iter().enumerate().skip(1).find(|(_, line)| {
        let trimmed = line.trim();
        trimmed.len() >= ticks && trimmed.chars().all(|value| value == '`')
    });

    match closing {
        Some((index, _)) => {
            // 闭合围栏后面还有正文，说明这不是「整段被围栏包住」，
            // 而是正文里夹了代码块。硬拆会把正文搅烂，宁可原样返回。
            if lines[index + 1..]
                .iter()
                .any(|line| !line.trim().is_empty())
            {
                return None;
            }
            Some(lines[1..index].join("\n"))
        }
        // 只有开围栏，通常是生成被截断。孤零零一行反引号绝不会是正文，直接丢。
        None => Some(lines[1..].join("\n")),
    }
}

fn strip_shell(text: &str) -> String {
    let mut current = text.trim_start().to_string();
    // 寒暄可能叠好几层（「好的，」换行再接「以下是回复：」），逐层剥到不再变化为止。
    for _ in 0..3 {
        let stripped = strip_shell_once(&current);
        if stripped == current {
            break;
        }
        current = stripped;
    }
    current
}

fn strip_shell_once(text: &str) -> String {
    let text = GREETING_LINE.replace(text, "");
    let text = replace_lead_in(&LEAD_IN_LINE, &text);
    let text = replace_lead_in(&LEAD_IN_INLINE, &text);
    strip_trailing_shell(&text)
}

fn replace_lead_in(pattern: &Regex, text: &str) -> String {
    match pattern.find(text) {
        Some(matched) if contains_any(matched.as_str(), LEAD_IN_MARKERS) => {
            text[matched.end()..].to_string()
        }
        _ => text.to_string(),
    }
}

fn strip_trailing_shell(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(last_break) = trimmed.rfind('\n') else {
        // 全文只有一行时不动：唯一一行既是壳也是正文，删了就没内容可发了。
        return text.to_string();
    };
    let last_line = trimmed[last_break + 1..].trim();
    let lowered = last_line.to_lowercase();
    if starts_with_any(&lowered, TRAILING_SHELL_STARTERS)
        && contains_any(&lowered, TRAILING_SHELL_MARKERS)
    {
        return trimmed[..last_break].trim_end().to_string();
    }
    text.to_string()
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn starts_with_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.starts_with(needle))
}

fn weak_refusal_hit(head: &str, lead: &str) -> bool {
    let Some(position) = head.find(lead) else {
        return false;
    };
    let gap = if lead.is_ascii() {
        REFUSAL_GAP_ASCII
    } else {
        REFUSAL_GAP_CJK
    };
    let window = head[position + lead.len()..]
        .chars()
        .take(gap)
        .collect::<String>();
    contains_any(&window, REFUSAL_OBJECTS)
}

#[cfg(test)]
mod tests {
    /// 断句点靠前时不能为了「断得干净」把正文砍掉大半
    #[test]
    fn an_early_sentence_break_does_not_gut_the_body() {
        let text = format!("您好。{}", "我熟悉该领域并有相关项目经验".repeat(6));
        let truncated = super::truncate_at_sentence(&text, 40);

        assert!(
            truncated.chars().count() > 24,
            "断句点在第 3 字，不该只留下「您好。」，实际：{truncated}"
        );
    }

    /// 断句点落在合理位置时仍应优先断句，而不是硬截出半个词
    #[test]
    fn a_late_sentence_break_is_still_preferred() {
        let text = "第一句话写得比较长所以占了不少字数。第二句还没写完就超了";
        let truncated = super::truncate_at_sentence(text, 22);

        assert!(truncated.ends_with('。'), "实际：{truncated}");
    }

    use super::{
        extract_json, has_placeholder, looks_like_refusal, sanitize, truncate_at_sentence,
    };

    #[test]
    fn strips_paired_reasoning_blocks() {
        assert_eq!(
            sanitize("<think>先分析一下</think>您好，我对这个岗位很感兴趣"),
            "您好，我对这个岗位很感兴趣"
        );
        assert_eq!(
            sanitize("<Thinking>A</THINKING>正文<reasoning>B</reasoning>结尾"),
            "正文结尾"
        );
    }

    #[test]
    fn strips_unclosed_reasoning_tag_to_end_of_text() {
        assert_eq!(
            sanitize("您好，我看到贵司的岗位<think>还没想完就被截断了"),
            "您好，我看到贵司的岗位"
        );
        assert_eq!(sanitize("<think>整段都是思考，没有闭合"), "");
    }

    #[test]
    fn strips_orphan_closing_reasoning_tag() {
        assert_eq!(
            sanitize("这段是我的推理过程，用户想要一段打招呼语</think>您好，我有三年后端经验"),
            "您好，我有三年后端经验"
        );
        assert_eq!(sanitize("推理\n换行推理\n</Think>\n正文"), "正文");
    }

    #[test]
    fn keeps_body_between_orphan_close_and_orphan_open() {
        assert_eq!(
            sanitize("前置思考</think>正文内容<think>后续思考"),
            "正文内容"
        );
    }

    #[test]
    fn unwraps_whole_text_code_fence() {
        assert_eq!(sanitize("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(sanitize("```\n您好\n```"), "您好");
        assert_eq!(sanitize("```json\n{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn unwraps_nested_code_fence() {
        let raw = "````\n```json\n{\"a\":1}\n```\n````";
        assert_eq!(sanitize(raw), "{\"a\":1}");
    }

    #[test]
    fn keeps_text_with_multiple_code_fences_intact() {
        // 两段并列的围栏说明正文里夹着代码块，拆外层会把中间的说明文字搅烂。
        let raw = "```json\n{\"a\":1}\n```\n说明文字\n```json\n{\"b\":2}\n```";
        assert_eq!(sanitize(raw), raw);
    }

    #[test]
    fn strips_lead_in_and_greeting_shell() {
        assert_eq!(
            sanitize("好的，以下是我为您生成的回复：\n您好，我对这个岗位很感兴趣"),
            "您好，我对这个岗位很感兴趣"
        );
        assert_eq!(sanitize("回复如下：\n您好"), "您好");
        assert_eq!(sanitize("好的，\n您好"), "您好");
        assert_eq!(sanitize("以下是为您准备的打招呼内容：您好"), "您好");
    }

    #[test]
    fn strips_lead_in_wrapping_a_code_fence() {
        assert_eq!(
            sanitize("好的，以下是生成的内容：\n```json\n{\"greeting\":\"你好\"}\n```"),
            "{\"greeting\":\"你好\"}"
        );
    }

    #[test]
    fn keeps_body_that_only_looks_like_a_shell() {
        // 「好的，」后面直接跟正文时是真话，不是壳。
        assert_eq!(sanitize("好的，我下周一可以入职"), "好的，我下周一可以入职");
        // 「以下是……：」自指的是候选人自己的经历，属于正文结构。
        let raw = "以下是我的项目经历：\n1. 支付系统重构";
        assert_eq!(sanitize(raw), raw);
    }

    #[test]
    fn strips_trailing_shell_only_when_self_referential() {
        assert_eq!(
            sanitize("您好，我有三年后端经验。\n希望这个回复对您有帮助！"),
            "您好，我有三年后端经验。"
        );
        assert_eq!(sanitize("您好。\n如需调整请随时告诉我。"), "您好。");
        let raw = "您好。\n如果贵司需要，我可以随时到岗，希望能有机会进一步沟通。";
        assert_eq!(sanitize(raw), raw);
    }

    #[test]
    fn collapses_blank_lines_and_trims() {
        assert_eq!(sanitize("  您好\n\n\n\n我是候选人  "), "您好\n\n我是候选人");
        assert_eq!(sanitize("您好\r\n\r\n我是候选人"), "您好\n\n我是候选人");
    }

    #[test]
    fn extracts_json_before_trailing_prose_with_braces() {
        let text = "{\"score\":90}\n上面的结果里 } 是收尾括号，仅供说明{不配对";
        assert_eq!(extract_json(text), Some("{\"score\":90}"));
    }

    #[test]
    fn extracts_json_with_braces_and_escaped_quotes_inside_strings() {
        let text = "前缀说明 {\"tpl\":\"{{name}} 说 \\\"}\\\" 结束{\",\"n\":{\"k\":1}} 后缀说明}";
        let extracted = extract_json(text).expect("json extracted");
        assert_eq!(
            extracted,
            "{\"tpl\":\"{{name}} 说 \\\"}\\\" 结束{\",\"n\":{\"k\":1}}"
        );
        let parsed = serde_json::from_str::<serde_json::Value>(extracted).expect("valid json");
        assert_eq!(parsed["n"]["k"], 1);
    }

    #[test]
    fn extracts_nested_json_and_handles_multibyte_prefix() {
        let text = "模型说明：{\"a\":{\"b\":[{\"c\":\"中文\"}]}}，以上。";
        let extracted = extract_json(text).expect("json extracted");
        assert_eq!(extracted, "{\"a\":{\"b\":[{\"c\":\"中文\"}]}}");
        serde_json::from_str::<serde_json::Value>(extracted).expect("valid json");
    }

    #[test]
    fn returns_none_when_json_is_absent_or_unbalanced() {
        assert_eq!(extract_json("完全没有花括号"), None);
        assert_eq!(extract_json("{\"a\":1"), None);
        assert_eq!(extract_json("{\"a\":\"没闭合的字符串}"), None);
    }

    #[test]
    fn detects_refusal_phrases() {
        assert!(looks_like_refusal("作为一个AI，我不能帮你做这件事"));
        assert!(looks_like_refusal("很抱歉，我无法提供这类内容"));
        assert!(looks_like_refusal("我不能提供该请求涉及的信息"));
        assert!(looks_like_refusal("As an AI, I cannot help with that"));
        assert!(looks_like_refusal("I'm unable to generate that message"));
        assert!(looks_like_refusal("I apologize, but this violates policy"));
    }

    #[test]
    fn does_not_flag_normal_replies_that_contain_refusal_words() {
        assert!(!looks_like_refusal(
            "我无法在下周入职，最快可以下个月初到岗"
        ));
        assert!(!looks_like_refusal("抱歉，刚才在开会没能及时回复"));
        assert!(!looks_like_refusal(
            "很抱歉，我无法在本周面试，改到下周可以吗"
        ));
        assert!(!looks_like_refusal(""));
        // 检测窗口之外出现的同类措辞属于正常语义，不算拒答。
        let prefix = "您好，感谢贵司的关注。我有三年后端开发经验，主导过支付系统重构，也做过高并发场景下的性能优化，希望能有机会与您进一步沟通交流。";
        assert!(prefix.chars().count() > 60);
        assert!(!looks_like_refusal(&format!(
            "{prefix}我无法提供更早的到岗时间"
        )));
    }

    #[test]
    fn detects_placeholders() {
        assert!(has_placeholder("我很认可XX公司的业务方向"));
        assert!(has_placeholder("我有X年相关经验"));
        assert!(has_placeholder("我有N年相关经验"));
        assert!(has_placeholder("曾就职于某公司"));
        assert!(has_placeholder("在某某担任负责人"));
        assert!(has_placeholder("您好，我想应聘[公司名]的岗位"));
        assert!(has_placeholder("这里是【待补充】的内容"));
        assert!(has_placeholder("请见 [请填写具体项目...]"));
        assert!(has_placeholder("您好 {{name}}"));
        assert!(has_placeholder("TODO: 补充项目经历"));
        assert!(has_placeholder("联系人 your_name"));
        assert!(has_placeholder("电话 138XXXX0000"));
    }

    #[test]
    fn does_not_flag_normal_text_containing_single_x() {
        assert!(!has_placeholder("我做过X光影像识别相关的项目"));
        assert!(!has_placeholder("熟悉Xbox平台的游戏开发"));
        assert!(!has_placeholder("技术栈包括[Java]与[Go]"));
        assert!(!has_placeholder("参考文献[1]中提到的算法"));
        assert!(!has_placeholder("我在某些场景下做过性能优化"));
        assert!(!has_placeholder("三年后端开发经验，主导过支付系统重构"));
    }

    #[test]
    fn truncate_returns_text_when_within_limit() {
        assert_eq!(truncate_at_sentence("您好。", 10), "您好。");
        // 恰好等于上限时原样返回，不做任何断句。
        assert_eq!(truncate_at_sentence("一二三四五", 5), "一二三四五");
    }

    #[test]
    fn truncate_breaks_at_last_sentence_end_for_chinese() {
        let text = "您好，我有三年后端经验。曾负责支付系统重构！还想再补充一点内容？剩下的会被截掉";
        let truncated = truncate_at_sentence(text, 20);
        assert_eq!(truncated, "您好，我有三年后端经验。");
        assert!(truncated.chars().count() <= 20);
        assert_eq!(
            truncate_at_sentence(text, 25),
            "您好，我有三年后端经验。曾负责支付系统重构！"
        );
    }

    #[test]
    fn truncate_hard_cuts_when_no_punctuation_exists() {
        let text = "一二三四五六七八九十";
        assert_eq!(truncate_at_sentence(text, 4), "一二三四");
        assert_eq!(truncate_at_sentence(text, 0), "");
    }

    #[test]
    fn truncate_never_splits_multibyte_characters() {
        let text = "中文👍表情符号混排的一段很长的文本内容";
        for limit in 0..text.chars().count() {
            let truncated = truncate_at_sentence(text, limit);
            assert!(truncated.chars().count() <= limit);
            assert!(text.starts_with(truncated.trim_end()));
        }
    }
}
