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
    let bounds = extract_bounds(main_part);
    if bounds.is_empty() {
        return None;
    }

    let (first_val, first_unit) = bounds[0];
    let (second_val, second_unit) = bounds.get(1).copied().unwrap_or(bounds[0]);

    if lower.contains("/天") || lower.contains("元/日") || lower.contains("/日") {
        let (mut min, mut max) = (first_val, second_val);
        if max < min {
            std::mem::swap(&mut min, &mut max);
        }
        return Some(ParsedSalary {
            unit: SalaryUnit::DailyYuan,
            min,
            max,
            months: 12,
        });
    }
    if lower.contains("/时") || lower.contains("小时") {
        let (mut min, mut max) = (first_val, second_val);
        if max < min {
            std::mem::swap(&mut min, &mut max);
        }
        return Some(ParsedSalary {
            unit: SalaryUnit::HourlyYuan,
            min,
            max,
            months: 12,
        });
    }

    let has_explicit_month = lower.contains("/月") || lower.contains("每月");
    let has_explicit_year =
        lower.contains("/年") || lower.contains("每年") || lower.contains("年薪");

    // 混合单位（如 `8千-1.2万`、`9千-1.5万/月`、`8k-1.2w`）：先按各自单位换算为 K，再排序
    if bounds.len() >= 2 {
        if let (Some(u1), Some(u2)) = (first_unit, second_unit) {
            if u1 != u2 {
                let mut min = scale_bound_to_k(first_val, u1);
                let mut max = scale_bound_to_k(second_val, u2);
                if max < min {
                    std::mem::swap(&mut min, &mut max);
                }
                if has_explicit_year && !has_explicit_month {
                    let divisor = months.max(12) as f32;
                    min /= divisor;
                    max /= divisor;
                }
                return Some(ParsedSalary {
                    unit: SalaryUnit::MonthlyK,
                    min,
                    max,
                    months,
                });
            }
        }
    }

    let (mut min, mut max) = (first_val, second_val);
    if max < min {
        std::mem::swap(&mut min, &mut max);
    }

    let resolved_unit = first_unit.or(second_unit).or_else(|| {
        if main_part.contains('万') || main_part.ends_with('w') {
            Some(BoundUnit::Wan)
        } else if main_part.contains('k') || main_part.contains('千') {
            Some(BoundUnit::K)
        } else if lower.contains("元/月") || lower.contains('元') {
            Some(BoundUnit::Yuan)
        } else {
            None
        }
    });

    match resolved_unit {
        Some(BoundUnit::Wan) => {
            // 显式 `/月` 必须优先于 `min >= 10` 年薪启发式（避免把 `10-15万/月` 误算为年薪）
            if has_explicit_month {
                return Some(ParsedSalary {
                    unit: SalaryUnit::MonthlyK,
                    min: min * 10.0,
                    max: max * 10.0,
                    months,
                });
            }
            // `万/年` / `w/年` 或无 `/月` 且 `>= 10 万/w` 按年薪折算为月薪 K
            if has_explicit_year || min >= 10.0 {
                let factor = 10.0 / (months.max(12) as f32);
                return Some(ParsedSalary {
                    unit: SalaryUnit::MonthlyK,
                    min: min * factor,
                    max: max * factor,
                    months,
                });
            }
            // `< 10 万/w`（如 `1.5-2.5万`、`1.5-2.5w·14薪`）按月薪折算为 K
            Some(ParsedSalary {
                unit: SalaryUnit::MonthlyK,
                min: min * 10.0,
                max: max * 10.0,
                months,
            })
        }
        Some(BoundUnit::K) => {
            if has_explicit_year && !has_explicit_month {
                let divisor = months.max(12) as f32;
                return Some(ParsedSalary {
                    unit: SalaryUnit::MonthlyK,
                    min: min / divisor,
                    max: max / divisor,
                    months,
                });
            }
            Some(ParsedSalary {
                unit: SalaryUnit::MonthlyK,
                min,
                max,
                months,
            })
        }
        Some(BoundUnit::Yuan) => {
            let divisor = if has_explicit_year && !has_explicit_month {
                1000.0 * (months.max(12) as f32)
            } else {
                1000.0
            };
            Some(ParsedSalary {
                unit: SalaryUnit::MonthlyK,
                min: min / divisor,
                max: max / divisor,
                months,
            })
        }
        None if has_explicit_month || min >= 1000.0 => Some(ParsedSalary {
            unit: SalaryUnit::MonthlyK,
            min: min / 1000.0,
            max: max / 1000.0,
            months,
        }),
        None => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundUnit {
    K,
    Wan,
    Yuan,
}

fn scale_bound_to_k(value: f32, unit: BoundUnit) -> f32 {
    match unit {
        BoundUnit::K => value,
        BoundUnit::Wan => value * 10.0,
        BoundUnit::Yuan => value / 1000.0,
    }
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

fn extract_bounds(text: &str) -> Vec<(f32, Option<BoundUnit>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut results = Vec::new();
    let mut i = 0;

    while i < chars.len() && results.len() < 2 {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut num = String::new();
        let mut seen_dot = false;
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_ascii_digit() {
                num.push(ch);
                i += 1;
            } else if ch == '.' && !seen_dot {
                seen_dot = true;
                num.push(ch);
                i += 1;
            } else {
                break;
            }
        }
        let Ok(value) = num.parse::<f32>() else {
            continue;
        };

        let mut look = i;
        while look < chars.len() && chars[look].is_whitespace() {
            look += 1;
        }
        let unit = chars.get(look).and_then(|ch| match ch {
            'k' | '千' => Some(BoundUnit::K),
            '万' | 'w' => Some(BoundUnit::Wan),
            '元' => Some(BoundUnit::Yuan),
            _ => None,
        });
        if unit.is_some() {
            i = look + 1;
        }
        results.push((value, unit));
    }

    results
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
    fn explicit_monthly_wan_beats_annual_heuristic() {
        let high_monthly = parse_salary("10-15万/月").unwrap();
        assert_eq!(high_monthly.unit, SalaryUnit::MonthlyK);
        assert!((high_monthly.min - 100.0).abs() < 0.01);
        assert!((high_monthly.max - 150.0).abs() < 0.01);
    }

    #[test]
    fn parses_mixed_qian_and_wan_units_without_corrupting_range() {
        let mixed = parse_salary("8千-1.2万").unwrap();
        assert_eq!(mixed.unit, SalaryUnit::MonthlyK);
        assert!((mixed.min - 8.0).abs() < 0.01);
        assert!((mixed.max - 12.0).abs() < 0.01);

        let mixed_monthly = parse_salary("9千-1.5万/月").unwrap();
        assert_eq!(mixed_monthly.unit, SalaryUnit::MonthlyK);
        assert!((mixed_monthly.min - 9.0).abs() < 0.01);
        assert!((mixed_monthly.max - 15.0).abs() < 0.01);
    }

    #[test]
    fn parses_w_suffix_inside_main_part_and_annual_w_heuristic() {
        let w_monthly = parse_salary("1.5-2.5w·14薪").unwrap();
        assert_eq!(w_monthly.unit, SalaryUnit::MonthlyK);
        assert!((w_monthly.min - 15.0).abs() < 0.01);
        assert!((w_monthly.max - 25.0).abs() < 0.01);
        assert_eq!(w_monthly.months, 14);

        let w_annual = parse_salary("20-35w").unwrap();
        assert_eq!(w_annual.unit, SalaryUnit::MonthlyK);
        assert!((w_annual.min - (200.0 / 12.0)).abs() < 0.02);
        assert!((w_annual.max - (350.0 / 12.0)).abs() < 0.02);
        assert_eq!(w_annual.months, 12);
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
