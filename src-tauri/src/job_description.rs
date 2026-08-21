//! 岗位描述原文的清洗与结构化。
//!
//! `JobDetail.detail` 存的是 RPA 抓页面时的 `textContent` 原文，没做过任何加工：
//!
//! - BOSS 混着反爬注入——插在正文中间的 `来自BOSS直聘`/`kanzhun`、
//!   整段 `.xxx{display:none}` 的样式代码、开头粘成一坨的技能标签，
//!   末尾还粘着招聘者名片、App 引导和工作地址。
//! - 猎聘存的根本不是 JD，而是列表页的岗位卡片文本，字段之间只有空格。
//!
//! **全应用只有这一份实现**：喂给模型的 prompt、岗位过滤的关键词匹配、
//! 前端的详情展示都从这里出去。规则跟着平台页面结构走，改一处就够，
//! 不必在前后端各维护一套彼此漂移的清洗逻辑。
//!
//! 清洗只发生在读取侧，库里始终留着原文：页面改版时重新解析一遍就行，
//! 不需要把已经抓到的岗位再抓一次。

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

/// 正文里的小节标题。命中即另起一节，顺序无关
const SECTION_TITLES: &[&str] = &[
    "职位描述",
    "岗位描述",
    "职位详情",
    "岗位详情",
    "岗位职责",
    "工作职责",
    "工作内容",
    "职位要求",
    "任职要求",
    "岗位要求",
    "任职资格",
    "能力要求",
    "加分项",
    "福利待遇",
    "薪资福利",
    "我们提供",
];

/// BOSS 反爬往正文里插的噪声词，直接抹掉
static BOSS_NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"来自BOSS直聘|BOSS直聘|kanzhun").expect("valid regex"));

/// 反爬用的内联样式块，如 `.HsDyPBbi{display:inline-block;...}`。
/// 字符类写死成 ASCII：`\w` 在 Rust 正则里是 Unicode 的，会把紧随其后的中文一起吞掉
static CSS_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.[A-Za-z_][A-Za-z0-9_-]*\s*\{[^}]*\}").expect("valid regex"));

/// 招聘者名片：`韩璐浓 在线 字节跳动 · HR.招聘专员`，粘在正文末尾
static BOSS_RECRUITER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s(\S{1,12})\s+(\S*(?:在线|活跃))\s+(\S.*?)\s+·\s+(\S.*?)\s*$")
        .expect("valid regex")
});

static BOSS_WORKPLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"工作地址\s*(.+?)\s*点击查看地图").expect("valid regex"));

/// 行首编号：`1、` `2.` `3）` `①` `- ` 等，剥掉后交给列表渲染
static LEADING_BULLET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:[（(]?[0-9]+[）)、.:：]|[-•·*]|[①-⑳])\s*").expect("valid regex")
});

/// 正文结束的位置：这之后全是 App 引导、工作地址和「查看更多」
const BOSS_TAIL_MARKER: &str = "去App与BOSS随时沟通";
/// 抓下来的文本一定以它开头，后面跟着页面上的技能标签
const BOSS_HEAD_NOISE: &str = "举报微信扫码分享";

const LIEPIN: &str = "liepin";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct JobSection {
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Recruiter {
    pub name: String,
    /// 「在线」「2周内活跃」这类活跃度描述
    pub status: String,
    pub company: String,
    pub role: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ParsedJobDescription {
    /// 结构化后的正文小节；没识别出标题时会有一个标题为空的兜底小节
    pub sections: Vec<JobSection>,
    /// 学历、经验、公司规模这类短标签，主要来自猎聘卡片
    pub highlights: Vec<String>,
    pub workplace: Option<String>,
    pub recruiter: Option<Recruiter>,
    /// 清洗后的正文全文。喂模型、做关键词匹配、前端「查看原文」都用它
    pub clean_text: String,
    /// 洗完没剩下有效内容——页面结构变了或本就没抓到 JD
    pub empty: bool,
}

/// 去掉行首编号；整行只有编号时返回空串，由调用方丢弃
fn strip_bullet(line: &str) -> String {
    LEADING_BULLET.replace(line, "").trim().to_string()
}

/// 命中的小节标题，允许尾随冒号
fn match_section_title(line: &str) -> Option<&'static str> {
    let normalized = line.trim_end_matches([':', '：', ' ']).trim();
    SECTION_TITLES
        .iter()
        .find(|title| **title == normalized)
        .copied()
}

/// 第一个独占一行的小节标题所在的字节偏移。
///
/// 两条抓取链路的噪声形态不同：岗位列表用 `textContent`，页面控件文本全糊在
/// 第一行；已沟通列表用 `innerText`，「举报」「微信扫码分享」会各占一行。
/// 后者靠摘第一行摘不干净，只能认准「BOSS 的 JD 必然从某个小节标题起头」这一点。
fn first_section_title_offset(text: &str) -> Option<usize> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if match_section_title(line.trim()).is_some() {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// 按小节标题把正文切开。标题之前的内容归到一个无标题小节，
/// 这样即使一个标题都没识别出来，正文也不会凭空消失
fn split_sections(text: &str) -> Vec<JobSection> {
    let mut sections = Vec::new();
    let mut current = JobSection::default();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(title) = match_section_title(line) {
            if !current.items.is_empty() {
                sections.push(std::mem::take(&mut current));
            }
            current = JobSection {
                title: title.to_string(),
                items: Vec::new(),
            };
            continue;
        }

        let item = strip_bullet(line);
        if !item.is_empty() {
            current.items.push(item);
        }
    }

    if !current.items.is_empty() {
        sections.push(current);
    }
    sections
}

fn parse_boss(detail: &str) -> ParsedJobDescription {
    let workplace = BOSS_WORKPLACE
        .captures(detail)
        .map(|caps| caps[1].trim().to_string())
        .filter(|value| !value.is_empty());

    // 尾巴先砍：App 引导之后没有任何 JD 内容，留着只会干扰招聘者名片的匹配
    let trimmed = match detail.find(BOSS_TAIL_MARKER) {
        Some(index) => &detail[..index],
        None => detail,
    };
    let without_css = CSS_BLOCK.replace_all(trimmed, "");
    let mut body = BOSS_NOISE.replace_all(&without_css, "").into_owned();

    // 招聘者名片没有换行，和正文最后一行粘在一起，只能从末尾反着认
    let recruiter = BOSS_RECRUITER.captures(&body).map(|caps| {
        let cut = caps.get(0).expect("whole match").start();
        let recruiter = Recruiter {
            name: caps[1].to_string(),
            status: caps[2].to_string(),
            company: caps[3].to_string(),
            role: caps[4].to_string(),
        };
        (recruiter, cut)
    });
    if let Some((_, cut)) = &recruiter {
        body.truncate(*cut);
    }
    let recruiter = recruiter.map(|(value, _)| value);

    // 第一行是「举报微信扫码分享 + 页面标题 + 粘成一坨的技能标签」，
    // 技能标签之间没有分隔符，切不出来也没必要留——正文里本就写着要求。
    // 但行尾那个小节标题是正文的一部分，得摘回来
    if let Some(break_at) = body.find('\n').filter(|index| *index > 0) {
        let (head, rest) = body.split_at(break_at);
        body = match SECTION_TITLES.iter().find(|title| head.ends_with(**title)) {
            Some(anchor) => format!("{anchor}{rest}"),
            // 认不出标题就只摘掉固定前缀：宁可留点噪声，也不能把正文首行一起丢了
            None => format!(
                "{}{rest}",
                head.strip_prefix(BOSS_HEAD_NOISE).unwrap_or(head)
            ),
        };
    }

    // 一个小节标题都认不出来时不截：那多半是格式特殊的 JD，
    // 宁可留点控件文本，也不能把整段正文当噪声丢了
    if let Some(start) = first_section_title_offset(&body) {
        body.drain(..start);
    }

    let clean_text = body.trim().to_string();
    ParsedJobDescription {
        sections: split_sections(&clean_text),
        highlights: Vec::new(),
        workplace,
        recruiter,
        empty: clean_text.is_empty(),
        clean_text,
    }
}

static LIEPIN_EXPERIENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"应届|经验不限|[0-9]+-[0-9]+年|[0-9]+年以[上下]").expect("valid regex")
});
static LIEPIN_EDUCATION: LazyLock<Regex> = LazyLock::new(|| {
    // 长的写在前面：交替是最左优先的，`本科` 排在 `统招本科` 前会把前缀吃掉
    Regex::new(r"统招本科|本科|大专|硕士|博士|中专|高中|学历不限|EMBA|MBA").expect("valid regex")
});
static LIEPIN_SCALE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"少于[0-9]+人|[0-9]+-[0-9]+人|[0-9]+人以[上下]|[0-9]+人").expect("valid regex")
});
/// 卡片末尾的活跃度，如 `1天前在线`、`23分钟前在线`
static LIEPIN_ACTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\S*(?:在线|活跃))\s*$").expect("valid regex"));
/// 招聘者：`杨女士·人事专员`
static LIEPIN_RECRUITER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\S+?)·(\S+)").expect("valid regex"));

/// 猎聘存的是列表卡片文本，字段之间只有空格：
/// `AI开发工程师 【 广州-黄埔区 】 8-15k 1-3年 本科 西麦科技 计算机软件新三板上市100-499人 杨女士·人事专员 1天前在线`
///
/// 这里只挑岗位表格里没有的字段（经验、学历、公司规模、招聘者），
/// 标题薪资地点已经是独立字段，重复展示没意义
fn parse_liepin(detail: &str) -> ParsedJobDescription {
    let text = detail.trim();
    if text.is_empty() {
        return ParsedJobDescription {
            empty: true,
            ..Default::default()
        };
    }

    let mut highlights: Vec<String> = Vec::new();
    for pattern in [&*LIEPIN_EXPERIENCE, &*LIEPIN_EDUCATION, &*LIEPIN_SCALE] {
        if let Some(found) = pattern.find(text) {
            let value = found.as_str().to_string();
            if !highlights.contains(&value) {
                highlights.push(value);
            }
        }
    }

    let recruiter = LIEPIN_RECRUITER.captures(text).map(|caps| Recruiter {
        name: caps[1].to_string(),
        role: caps[2].to_string(),
        status: LIEPIN_ACTIVE
            .captures(text)
            .map(|active| active[1].to_string())
            .unwrap_or_default(),
        company: String::new(),
    });

    ParsedJobDescription {
        sections: Vec::new(),
        empty: highlights.is_empty() && recruiter.is_none(),
        highlights,
        workplace: None,
        recruiter,
        clean_text: text.to_string(),
    }
}

/// 把抓下来的岗位描述原文洗成可渲染、可喂模型的结构。
///
/// 平台没标注时按 BOSS 处理：存量数据里 BOSS 占九成，且 BOSS 的清洗规则
/// 对普通文本是幂等的，误判的代价只是少洗掉几处噪声。
pub fn parse(detail: &str, platform: &str) -> ParsedJobDescription {
    if detail.trim().is_empty() {
        return ParsedJobDescription {
            empty: true,
            ..Default::default()
        };
    }
    if platform == LIEPIN {
        parse_liepin(detail)
    } else {
        parse_boss(detail)
    }
}

/// 只要清洗后的正文。喂 prompt、做关键词匹配都用这个出口，
/// 免得每个调用方各自决定「要不要洗」「洗到什么程度」
pub fn clean_text(detail: &str, platform: &str) -> String {
    parse(detail, platform).clean_text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 结构取自真实抓取结果，保留了反爬注入的原样
    const BOSS_RAW: &str = concat!(
        "举报微信扫码分享职位描述GolangJavaC++CMySQLAI agentSpringPython",
        ".HsDyPBbi{display:inline-block;width:0.1px;height:0.1px;overflow:hidden;visibility: hidden;}",
        ".nfJJHrefm{display:none!important;}",
        "来自BOSS直聘职位描述\n",
        "1、深入理解来自BOSS直聘客户业务场景，快速开BOSS直聘发AI应用原型验BOSS直聘证，kanzhun不断迭代推进至生产级部署的Agent系统；\n",
        "2、基于AI和云设计架构，并通过AI辅助开发在客户环境中快速实现；\n",
        "职位要求\n",
        "1、本科及以上学历，计算机、工程、数学等相关专业或同等实践经验；\n",
        "2、3年以上研发工作经验；\n",
        "\n",
        "加分项\n",
        "1、有复杂Agent系统的开发经验。 韩璐浓 在线 字节跳动 · HR.招聘专员  ",
        "去App与BOSS随时沟通 前往App与BOSS随时沟通工作地址深圳南山区深圳景湖大厦",
        "广东省深圳市南山区创业路1555号景湖大厦点击查看地图查看更多信息",
    );

    #[test]
    fn strips_anti_scraping_noise_from_boss_detail() {
        let parsed = parse(BOSS_RAW, "boss");

        assert!(!parsed.clean_text.contains("display:inline-block"));
        assert!(!parsed.clean_text.contains("来自BOSS直聘"));
        assert!(!parsed.clean_text.contains("kanzhun"));
        // 噪声词是插在句子中间的，抹掉后语义要接得上
        assert!(parsed
            .clean_text
            .contains("快速开发AI应用原型验证，不断迭代"));
    }

    #[test]
    fn drops_glued_skill_tags_but_keeps_the_section_title_behind_them() {
        let parsed = parse(BOSS_RAW, "boss");

        assert!(!parsed.clean_text.contains("举报微信扫码分享"));
        assert!(!parsed.clean_text.contains("GolangJavaC++"));
        assert_eq!(parsed.sections[0].title, "职位描述");
    }

    #[test]
    fn splits_sections_and_strips_leading_numbers() {
        let parsed = parse(BOSS_RAW, "boss");

        let titles: Vec<&str> = parsed
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(titles, vec!["职位描述", "职位要求", "加分项"]);
        assert_eq!(parsed.sections[0].items.len(), 2);
        assert!(parsed.sections[0].items[0].starts_with("深入理解"));
        assert_eq!(
            parsed.sections[2].items,
            vec!["有复杂Agent系统的开发经验。"]
        );
    }

    #[test]
    fn extracts_recruiter_card_glued_to_the_last_line() {
        let parsed = parse(BOSS_RAW, "boss");

        assert_eq!(
            parsed.recruiter,
            Some(Recruiter {
                name: "韩璐浓".to_string(),
                status: "在线".to_string(),
                company: "字节跳动".to_string(),
                role: "HR.招聘专员".to_string(),
            })
        );
        // 名片摘走后不能残留在最后一条正文里
        assert!(!parsed.clean_text.contains("韩璐浓"));
    }

    #[test]
    fn extracts_workplace_without_dragging_app_banner_into_the_body() {
        let parsed = parse(BOSS_RAW, "boss");

        assert!(parsed.workplace.as_deref().unwrap().contains("景湖大厦"));
        assert!(!parsed.clean_text.contains("去App与BOSS随时沟通"));
        assert!(!parsed.clean_text.contains("查看更多信息"));
    }

    #[test]
    fn liepin_card_yields_conditions_and_recruiter() {
        let parsed = parse(
            "AI开发工程师 【 广州-黄埔区 】 8-15k 1-3年 本科 西麦科技 计算机软件新三板上市100-499人 杨女士·人事专员 1天前在线",
            "liepin",
        );

        assert_eq!(parsed.highlights, vec!["1-3年", "本科", "100-499人"]);
        assert_eq!(
            parsed.recruiter,
            Some(Recruiter {
                name: "杨女士".to_string(),
                role: "人事专员".to_string(),
                status: "1天前在线".to_string(),
                company: String::new(),
            })
        );
    }

    #[test]
    fn blank_detail_is_empty() {
        for value in ["", "   \n  "] {
            assert!(parse(value, "boss").empty);
            assert!(parse(value, "liepin").empty);
        }
    }

    #[test]
    fn body_survives_when_no_section_title_is_recognized() {
        let parsed = parse("负责后端服务开发\n参与架构设计", "boss");

        assert!(!parsed.empty);
        assert_eq!(parsed.sections.len(), 1);
        assert_eq!(parsed.sections[0].title, "");
        assert_eq!(
            parsed.sections[0].items,
            vec!["负责后端服务开发", "参与架构设计"]
        );
    }

    /// 已沟通列表走 innerText，「举报」「微信扫码分享」会各占一行落在 JD 前面
    #[test]
    fn drops_page_controls_stacked_above_the_first_section_title() {
        let parsed = parse(
            "微信扫码分享\n举\n报\n职位描述\n负责推荐系统后端开发。\n要求熟悉 Rust。",
            "boss",
        );

        assert_eq!(parsed.sections.len(), 1);
        assert_eq!(parsed.sections[0].title, "职位描述");
        assert_eq!(
            parsed.sections[0].items,
            vec!["负责推荐系统后端开发。", "要求熟悉 Rust。"]
        );
        assert!(!parsed.clean_text.contains("微信扫码分享"));
    }

    #[test]
    fn missing_platform_falls_back_to_boss_rules() {
        let parsed = parse("岗位职责\n1. 写代码", "");

        assert_eq!(parsed.sections[0].title, "岗位职责");
        assert_eq!(parsed.sections[0].items, vec!["写代码"]);
    }

    #[test]
    fn clean_text_is_the_single_exit_for_prompt_and_matching() {
        // 出口只有一个：prompt 与关键词匹配拿到的必须是同一份文本
        assert_eq!(clean_text(BOSS_RAW, "boss"), parse(BOSS_RAW, "boss").clean_text);
        assert!(!clean_text(BOSS_RAW, "boss").contains("kanzhun"));
    }
}
