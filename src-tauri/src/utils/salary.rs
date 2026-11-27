use serde::{Deserialize, Serialize};

// 这个映射怎么来的？
// Boss 岗位薪资做了反爬处理  salary的content 取出来的是字体编码 浏览器会根据特殊的字体去渲染 因此我们在浏览器上看是有薪资描述，但直接取 或者copy 会有乱码问题
// 字体库：src\resource\font\3kovsijnt11693967587313.woff2
// 去站点：https://www.bejson.com/ui/font/ 加载一下这个文件 就能获取到 每个编码对应的数字了
const FONT_MAP: &[(u32, char)] = &[
    (0xE031, '0'),
    (0xE032, '1'),
    (0xE033, '2'),
    (0xE034, '3'),
    (0xE035, '4'),
    (0xE036, '5'),
    (0xE037, '6'),
    (0xE038, '7'),
    (0xE039, '8'),
    (0xE03A, '9'),
];

pub fn decode_salary(text: &str) -> String {
    text.chars()
        .map(|c| {
            FONT_MAP
                .iter()
                .find_map(|(codepoint, decoded)| (*codepoint == c as u32).then_some(*decoded))
                .unwrap_or(c)
        })
        .collect()
}

/// 薪资的计薪周期单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SalaryUnit {
    /// 月薪（单位：千元/月，例如 15-25K）
    MonthlyK,
    /// 日薪（单位：元/天，例如 200-300元/天）
    DailyYuan,
    /// 时薪（单位：元/时）
    HourlyYuan,
    /// 面议或未提供数值
    Negotiable,
}

/// 解析后的结构化薪资，供本地真实薪资过滤与防虚标校验使用。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSalary {
    pub unit: SalaryUnit,
    /// 薪资下限（MonthlyK 单位为 K，DailyYuan/HourlyYuan 为元）
    pub min: f32,
    /// 薪资上限（与 min 同单位）
    pub max: f32,
    /// 年薪月数（如 14 薪），未标明时为 12
    pub months: u8,
}

impl ParsedSalary {
    /// 上限与下限的跨度倍数（用于识别 `8-20K` 等宽跨度虚标岗）
    pub fn span_ratio(&self) -> Option<f32> {
        if self.min > 0.0 && self.max >= self.min {
            Some(self.max / self.min)
        } else {
            None
        }
    }
}

/// 将 BOSS / 猎聘 / 51job 常见薪资文案解析为结构化数值。
///
/// 支持示例：
/// - `15-25K` / `15-25K·14薪` / `15k-25k`
/// - `20-35万/年` / `20-35万` -> 折算为月薪 K（÷ 12 × 10）
/// - `1.5-2.5万/月` / `1.5-2.5万` -> 折算为月薪 K（× 10）
/// - `8000-12000元/月` -> 折算为月薪 K（÷ 1000）
/// - `200-300元/天` / `30元/时`
/// - `面议` / `薪资面议`
pub fn parse_salary(raw: &str) -> Option<ParsedSalary> {
    let decoded = decode_salary(raw);
    let normalized = decoded.trim().replace(['～', '~', '—', '–'], "-");
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("面议") {
        return Some(ParsedSalary {
            unit: SalaryUnit::Negotiable,
            min: 0.0,
            max: 0.0,
            months: 12,
        });
    }

    let lower = normalized.to_ascii_lowercase();
    let months = extract_months(&lower).unwrap_or(12);
    // 剥离 `·14薪` / `14薪` 后再提取数字，防止把月数误当成薪资上限
    let main_part = lower.split(['·', '•']).next().unwrap_or(&lower);
    let main_part = strip_trailing_months(main_part);
    let (mut min, mut max) = extract_range(main_part)?;
    if max < min {
        std::mem::swap(&mut min, &mut max);
    }

    if lower.contains("/天") || lower.contains("元/日") || lower.contains("/日") {
        return Some(ParsedSalary {
            unit: SalaryUnit::DailyYuan,
            min,
            max,
            months: 12,
        });
    }
    if lower.contains("/时") || lower.contains("小时") {
        return Some(ParsedSalary {
            unit: SalaryUnit::HourlyYuan,
            min,
            max,
            months: 12,
        });
    }

    // `万/年` 或大额 `万`（>= 10 万通常为年薪）折算为月薪 K
    if lower.contains("万/年") || lower.contains("w/年") || (lower.contains('万') && min >= 10.0) {
        let factor = 10.0 / (months.max(12) as f32);
        return Some(ParsedSalary {
            unit: SalaryUnit::MonthlyK,
            min: min * factor,
            max: max * factor,
            months,
        });
    }
    // `1.5-2.5万/月` 或 `< 10 万` 折算为月薪 K
    if lower.contains('万') || lower.contains("w/月") || lower.ends_with('w') {
        return Some(ParsedSalary {
            unit: SalaryUnit::MonthlyK,
            min: min * 10.0,
            max: max * 10.0,
            months,
        });
    }
    // `15-25k` / `15-25千`
    if lower.contains('k') || lower.contains('千') {
        return Some(ParsedSalary {
            unit: SalaryUnit::MonthlyK,
            min,
            max,
            months,
        });
    }
    // `8000-12000元/月` 或裸大数字按元/月折算为 K
    if lower.contains("元/月") || lower.contains('元') || min >= 1000.0 {
        return Some(ParsedSalary {
            unit: SalaryUnit::MonthlyK,
            min: min / 1000.0,
            max: max / 1000.0,
            months,
        });
    }

    None
}

fn extract_months(text: &str) -> Option<u8> {
    let idx = text.find('薪')?;
    let prefix = &text[..idx];
    let digits: String = prefix
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let parsed: u8 = digits.parse().ok()?;
    (12..=36).contains(&parsed).then_some(parsed)
}

fn strip_trailing_months(text: &str) -> &str {
    if let Some(idx) = text.find('薪') {
        let prefix = &text[..idx];
        let trimmed = prefix.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());
        if trimmed.len() < prefix.len() {
            return trimmed;
        }
    }
    text
}

fn extract_range(text: &str) -> Option<(f32, f32)> {
    let mut numbers: Vec<f32> = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !current.is_empty() && !current.contains('.')) {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<f32>() {
                numbers.push(value);
            }
            current.clear();
            if numbers.len() >= 2 {
                break;
            }
        }
    }
    if !current.is_empty() && numbers.len() < 2 {
        if let Ok(value) = current.parse::<f32>() {
            numbers.push(value);
        }
    }

    match numbers.as_slice() {
        [single] => Some((*single, *single)),
        [first, second, ..] => Some((*first, *second)),
        [] => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_boss_salary_text_from_known_font_mapping() {
        let encoded_salary = "\u{e038}\u{e032}-\u{e035}\u{e039}K·13薪";

        let decoded = decode_salary(encoded_salary);

        assert_eq!(decoded, "71-48K·13薪");
    }

    #[test]
    fn parses_monthly_k_with_and_without_bonus_months() {
        let parsed = parse_salary("20-35K·14薪").unwrap();
        assert_eq!(parsed.unit, SalaryUnit::MonthlyK);
        assert_eq!(parsed.min, 20.0);
        assert_eq!(parsed.max, 35.0);
        assert_eq!(parsed.months, 14);
        assert!((parsed.span_ratio().unwrap() - 1.75).abs() < 0.01);

        let plain = parse_salary("15k-30k").unwrap();
        assert_eq!(plain.min, 15.0);
        assert_eq!(plain.max, 30.0);
        assert_eq!(plain.months, 12);
    }

    #[test]
    fn parses_51job_wan_per_month_and_annual_formats() {
        let monthly = parse_salary("1.5-2.5万/月").unwrap();
        assert_eq!(monthly.unit, SalaryUnit::MonthlyK);
        assert!((monthly.min - 15.0).abs() < 0.01);
        assert!((monthly.max - 25.0).abs() < 0.01);

        let annual = parse_salary("24-36万/年").unwrap();
        assert_eq!(annual.unit, SalaryUnit::MonthlyK);
        assert!((annual.min - 20.0).abs() < 0.01);
        assert!((annual.max - 30.0).abs() < 0.01);
    }

    #[test]
    fn parses_daily_intern_and_negotiable_salaries() {
        let daily = parse_salary("200-300元/天").unwrap();
        assert_eq!(daily.unit, SalaryUnit::DailyYuan);
        assert_eq!(daily.min, 200.0);
        assert_eq!(daily.max, 300.0);

        let negotiable = parse_salary("薪资面议").unwrap();
        assert_eq!(negotiable.unit, SalaryUnit::Negotiable);
    }
}
