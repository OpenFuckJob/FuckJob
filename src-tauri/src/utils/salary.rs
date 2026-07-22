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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_boss_salary_text_from_known_font_mapping() {
        let encoded_salary = "\u{e038}\u{e032}-\u{e035}\u{e039}K·13薪";

        let decoded = decode_salary(encoded_salary);

        assert_eq!(decoded, "71-48K·13薪");
    }
}
