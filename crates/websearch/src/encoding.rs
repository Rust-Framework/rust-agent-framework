//! 字符编码检测与转换。
//!
//! 基于 `encoding_rs`（Firefox 同款编码库）自动检测网页编码，
//! 解决中文站点常见的 GBK/GB2312/GB18030 编码问题。

use encoding_rs::Encoding;

/// 常见的中文编码嗅探器检测顺序。
///
/// 当 Content-Type 没有指定 charset 或指定的 charset 解码失败时，
/// 按此顺序尝试解码。
const CN_ENCODINGS: &[&str] = &[
    "gbk", "gb18030", "gb2312", "big5", "utf-8",
];

/// 从原始字节解码为 UTF-8 字符串。
///
/// ## 解码策略
///
/// 1. 检测 BOM（字节序标记），如果存在则直接用对应编码解码
/// 2. 如果已知 `declared_charset`（来自 HTTP Content-Type header 或 HTML meta），
///    优先使用声明的编码解码
/// 3. 如果没有任何线索，先尝试 UTF-8，再按中文编码优先级尝试
/// 4. 最终降级为 UTF-8 lossy
pub fn decode_bytes(bytes: &[u8], declared_charset: Option<&str>) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // ── 策略 1: BOM 检测 ──
    if let Some((encoding, _bom_len)) = Encoding::for_bom(bytes) {
        let (decoded, _, _) = encoding.decode(bytes);
        return decoded.into_owned();
    }

    // ── 策略 2: 使用声明的 charset ──
    if let Some(charset) = declared_charset {
        let charset_lower = charset.to_lowercase();
        if let Some(decoded) = try_decode_with(bytes, &charset_lower) {
            // 检查是否有过多替换字符（说明编码可能不对）
            let replacement_count = decoded.chars().filter(|&c| c == '\u{FFFD}').count();
            if replacement_count < decoded.len() / 20 && !decoded.is_empty() {
                return decoded;
            }
            // 太多替换字符，回退到自动检测
        }
    }

    // ── 策略 3: 先尝试 UTF-8 ──
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    // ── 策略 4: 按常见编码顺序尝试 ──
    for encoding_name in CN_ENCODINGS {
        if let Some(decoded) = try_decode_with(bytes, encoding_name) {
            if !decoded.trim().is_empty() {
                return decoded;
            }
        }
    }

    // ── 策略 5: 最后的降级 — 用 lossy UTF-8 ──
    String::from_utf8_lossy(bytes).to_string()
}

/// 用指定编码名解码。
fn try_decode_with(bytes: &[u8], encoding_name: &str) -> Option<String> {
    let encoding = Encoding::for_label_no_replacement(encoding_name.as_bytes())?;
    let (decoded, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        // 有少量错误可以接受，检查替换字符比例
        let s = decoded.into_owned();
        let replacement_count = s.chars().filter(|&c| c == '\u{FFFD}').count();
        if replacement_count > s.len() / 10 {
            return None;
        }
        Some(s)
    } else {
        Some(decoded.into_owned())
    }
}

/// 从 HTTP Content-Type header 中提取 charset。
///
/// 示例:
/// - `text/html; charset=gbk` → `Some("gbk")`
/// - `text/html` → `None`
pub fn parse_content_type_charset(content_type: &str) -> Option<String> {
    let ct_lower = content_type.to_lowercase();
    for part in ct_lower.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("charset=") {
            let charset = value.trim().trim_matches('"').trim_matches('\'');
            if !charset.is_empty() {
                return Some(charset.to_string());
            }
        }
    }
    None
}

/// 从 HTML meta 标签中提取 charset。
///
/// 支持两种格式:
/// - `<meta charset="gbk">`
/// - `<meta http-equiv="Content-Type" content="text/html; charset=gbk">`
pub fn parse_meta_charset(html_bytes: &[u8]) -> Option<String> {
    // 只用前 1024 字节查找 meta（一般都在头部）
    let head = if html_bytes.len() > 1024 {
        &html_bytes[..1024]
    } else {
        html_bytes
    };
    let head_str = String::from_utf8_lossy(head);

    // 格式 1: <meta charset="gbk">
    let meta_lower = head_str.to_lowercase();
    if let Some(pos) = meta_lower.find("<meta") {
        let tag = &meta_lower[pos..];
        if let Some(end) = tag.find('>') {
            let tag = &tag[..=end];
            if let Some(charset_pos) = tag.find("charset=") {
                let after = &tag[charset_pos + 8..];
                let value = after
                    .trim_start()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .split(|c| c == '"' || c == '\'' || c == '>' || c == ' ' || c == '/')
                    .next()
                    .unwrap_or("");
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_utf8() {
        let input = "Hello, 世界!".as_bytes();
        let result = decode_bytes(input, Some("utf-8"));
        assert_eq!(result, "Hello, 世界!");
    }

    #[test]
    fn test_decode_gbk() {
        // "你好世界" in GBK
        let gbk_bytes = vec![0xc4, 0xe3, 0xba, 0xc3, 0xca, 0xc0, 0xbd, 0xe7];
        let result = decode_bytes(&gbk_bytes, None);
        assert_eq!(result, "你好世界");
    }

    #[test]
    fn test_decode_gbk_with_declared_charset() {
        let gbk_bytes = vec![0xc4, 0xe3, 0xba, 0xc3, 0xca, 0xc0, 0xbd, 0xe7];
        let result = decode_bytes(&gbk_bytes, Some("gbk"));
        assert_eq!(result, "你好世界");
    }

    #[test]
    fn test_parse_content_type_charset() {
        assert_eq!(
            parse_content_type_charset("text/html; charset=gbk"),
            Some("gbk".to_string())
        );
        assert_eq!(
            parse_content_type_charset("text/html; charset=utf-8"),
            Some("utf-8".to_string())
        );
        assert_eq!(
            parse_content_type_charset(r#"text/html; charset="utf-8""#),
            Some("utf-8".to_string())
        );
        assert_eq!(parse_content_type_charset("text/html"), None);
    }

    #[test]
    fn test_parse_meta_charset() {
        assert_eq!(
            parse_meta_charset(r#"<meta charset="gbk">"#.as_bytes()),
            Some("gbk".to_string())
        );
        assert_eq!(
            parse_meta_charset(r#"<meta http-equiv="Content-Type" content="text/html; charset=gb2312">"#.as_bytes()),
            Some("gb2312".to_string())
        );
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode_bytes(b"", None), "");
    }
}
