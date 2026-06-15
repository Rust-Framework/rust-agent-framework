//! HTML 工具函数：解码、清理、URL 解析等。

/// 解码常见的 HTML 实体。
pub fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// 去除 HTML 标签，返回纯文本。
pub fn strip_html_tags(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_tag = false;

    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

/// 清理文本：去除 HTML 标签 + HTML 实体解码 + 合并空白。
pub fn clean_html(input: &str) -> String {
    let stripped = strip_html_tags(input);
    let decoded = decode_html_entities(&stripped);
    // 合并连续空白
    let mut result = String::with_capacity(decoded.len());
    let mut last_was_whitespace = false;

    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !last_was_whitespace {
                result.push(' ');
                last_was_whitespace = true;
            }
        } else {
            result.push(ch);
            last_was_whitespace = false;
        }
    }

    result.trim().to_string()
}

/// 解析 DuckDuckGo 跳转 URL（`/l/?uddg=...` 格式）。
/// 返回解码后的真实目标 URL。
pub fn resolve_duckduckgo_url(url: &str) -> String {
    let decoded = decode_html_entities(url);

    // 处理 /l/?uddg=... 跳转链接
    if let Some(rest) = decoded.strip_prefix("/l/?uddg=") {
        return urlencoding_decode(rest).unwrap_or_else(|| decoded.clone());
    }

    // 协议相对 URL（如 //example.com/page）
    if let Some(rest) = decoded.strip_prefix("//") {
        return format!("https://{rest}");
    }

    decoded
}

/// 最小化的 URL 解码（%XX 格式）。
fn urlencoding_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        match ch {
            '%' => {
                let hi = chars.next()?;
                let lo = chars.next()?;
                let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
                bytes.push(byte);
            }
            '+' => bytes.push(b' '),
            c if c.is_ascii() => bytes.push(c as u8),
            _ => {} // 跳过非 ASCII
        }
    }

    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(decode_html_entities("&amp;"), "&");
        assert_eq!(decode_html_entities("a &lt; b"), "a < b");
        assert_eq!(decode_html_entities("a &gt; b"), "a > b");
        assert_eq!(decode_html_entities("&nbsp;hello"), " hello");
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(strip_html_tags("<a href='x'>link</a> text"), "link text");
    }

    #[test]
    fn test_clean_html() {
        assert_eq!(clean_html("  <b>Hello</b>  &amp;  World  "), "Hello & World");
    }

    #[test]
    fn test_resolve_duckduckgo_url_direct() {
        assert_eq!(
            resolve_duckduckgo_url("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn test_resolve_duckduckgo_url_protocol_relative() {
        assert_eq!(
            resolve_duckduckgo_url("//example.com/page"),
            "https://example.com/page"
        );
    }

    #[test]
    fn test_resolve_duckduckgo_url_redirect() {
        let encoded = urlencoding_encode("https://example.com/hello world");
        let redirect = format!("/l/?uddg={encoded}");
        assert_eq!(
            resolve_duckduckgo_url(&redirect),
            "https://example.com/hello world"
        );
    }

    fn urlencoding_encode(input: &str) -> String {
        let mut result = String::new();
        for byte in input.as_bytes() {
            match *byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(*byte as char);
                }
                b' ' => result.push('+'),
                _ => result.push_str(&format!("%{:02X}", byte)),
            }
        }
        result
    }
}
