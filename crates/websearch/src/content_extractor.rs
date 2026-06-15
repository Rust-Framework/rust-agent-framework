//! 网页正文提取模块。
//!
//! 实现 Readability 风格的正文提取算法：
//! 通过文本密度分析和标签评分，识别页面中的主要内容区域，
//! 自动去除导航栏、页脚、侧边栏、广告等噪音内容。

use scraper::{Html, Node, Selector};
use std::collections::HashSet;

/// 噪音标签集合：这些标签中的内容会被直接丢弃。
const NOISE_TAGS: &[&str] = &[
    "script", "style", "noscript", "iframe", "svg", "canvas",
    "video", "audio", "object", "embed", "applet", "form",
    "input", "textarea", "select", "button", "option", "optgroup",
];

/// 非正文内容标签（整体去除）。
const NON_CONTENT_TAGS: &[&str] = &[
    "nav", "footer", "header", "aside", "fieldset", "figure", "figcaption",
];

/// 经常用作非正文的 class/id 关键词。
const BAD_CONTENT_PATTERNS: &[&str] = &[
    "comment", "sidebar", "footer", "header", "nav", "menu",
    "advertisement", "ad-", "ad_", "-ad", "_ad", "sponsor",
    "breadcrumb", "pagination", "social", "share", "related",
    "recommend", "widget", "popup", "modal", "overlay",
    "cookie", "banner", "toolbar", "search-box", "searchform",
];

/// 经常是正文内容的 class/id 关键词。
const GOOD_CONTENT_PATTERNS: &[&str] = &[
    "article", "content", "post", "main", "entry", "body",
    "text", "detail", "news", "story", "blog", "document",
    "page-content", "post-content", "article-content", "entry-content",
    "main-content", "text-content",
];

/// 提取 HTML 页面的正文内容（纯文本）。
///
/// ## 算法流程
///
/// 1. 遍历 DOM 树，收集所有文本块节点及其属性
/// 2. 计算每个文本块的"内容评分"（文本密度 × 标签权重）
/// 3. 找到最高评分的容器，提取其中的文本
/// 4. 如果找不到合适的容器，退化为全局文本提取
pub fn extract_main_content(html: &str) -> String {
    let document = Html::parse_document(html);

    // 提取 title
    let title = extract_title(&document);

    // 策略 1: 使用正文容器提取
    if let Some(content) = extract_by_scoring(&document) {
        let result = clean_and_format(&content);
        if result.len() > 100 {
            return format_result(&title, &result);
        }
    }

    // 策略 2: 从已知正文选择器提取
    if let Some(content) = extract_by_known_selectors(&document) {
        let result = clean_and_format(&content);
        if result.len() > 50 {
            return format_result(&title, &result);
        }
    }

    // 策略 3: 全局 body 文本提取（降级方案）
    let body_text = extract_body_text(&document);
    format_result(&title, &body_text)
}

fn format_result(title: &str, content: &str) -> String {
    if title.is_empty() {
        content.to_string()
    } else {
        format!("Title: {title}\n\n{content}")
    }
}

/// 提取页面标题。
fn extract_title(document: &Html) -> String {
    Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default()
}

/// 策略 1: 评分法 — 找到文本密度最高的容器。
fn extract_by_scoring(document: &Html) -> Option<String> {
    let body = document
        .select(&Selector::parse("body").ok()?)
        .next()?;

    // 查找最佳候选容器
    let (best_element, _score) = find_best_content_container(body);

    // 提取选中容器中的文本
    let text = extract_element_text(&best_element);
    if text.trim().len() > 100 {
        return Some(text);
    }
    None
}

/// 递归遍历 DOM 树，找到正文内容最丰富的容器。
///
/// 使用自底向上的评分策略：
/// - 叶子文本节点直接返回其文本块
/// - 容器节点汇总子文本块，计算加权分数
fn find_best_content_container(node: scraper::ElementRef) -> (scraper::ElementRef, f64) {
    let mut best_child: Option<(scraper::ElementRef, f64)> = None;
    let mut total_text_len = 0usize;
    let mut total_link_text_len = 0usize;
    let mut child_count = 0usize;

    for child in node.children() {
        match child.value() {
            Node::Text(text) => {
                let t = text.text.trim();
                total_text_len += t.len();
            }
            Node::Element(_) => {
                if let Some(child_el) = scraper::ElementRef::wrap(child) {
                    let tag_name = child_el.value().name().to_lowercase();

                    // 跳过噪音标签
                    if NOISE_TAGS.contains(&tag_name.as_str()) {
                        continue;
                    }

                    // 递归处理子元素
                    let (_, score) = find_best_content_container(child_el);

                    // 收集该子元素的文本统计
                    let text = extract_element_text(&child_el);
                    let text_len = text.chars().filter(|c| !c.is_whitespace()).count();
                    let link_text_len = count_link_text(&child_el);

                    total_text_len += text_len;
                    total_link_text_len += link_text_len;
                    child_count += 1;

                    match &best_child {
                        None => best_child = Some((child_el, score)),
                        Some((_, s)) if score > *s => best_child = Some((child_el, score)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // 计算当前节点的内容评分
    let score = if total_text_len == 0 {
        0.0
    } else {
        let link_ratio = total_link_text_len as f64 / total_text_len.max(1) as f64;
        let text_density = total_text_len as f64;

        // 链接比例惩罚（正文通常链接密度低）
        let link_penalty = if link_ratio > 0.5 { 0.3 } else { 1.0 };

        // 子元素多样性奖励
        let child_bonus = (child_count as f64).min(5.0) * 0.1 + 1.0;

        text_density * link_penalty * child_bonus
    };

    // 标签权重加成
    let tag_name = node.value().name().to_lowercase();
    let class_str = node
        .value()
        .attr("class")
        .unwrap_or("")
        .to_lowercase();
    let id_str = node
        .value()
        .attr("id")
        .unwrap_or("")
        .to_lowercase();

    let tag_score = match tag_name.as_str() {
        "article" | "main" => 1.5,
        "section" => 1.2,
        "div" => 1.0,
        "p" => 0.8,
        "li" | "td" => 0.6,
        "span" | "a" => 0.3,
        _ => 0.5,
    };

    // class/id 关键词评分
    let combined_attrs = format!("{class_str} {id_str}");
    let attr_score = if GOOD_CONTENT_PATTERNS.iter().any(|p| combined_attrs.contains(p)) {
        1.5
    } else if BAD_CONTENT_PATTERNS.iter().any(|p| combined_attrs.contains(p)) {
        0.3
    } else {
        1.0
    };

    let final_score = score * tag_score * attr_score;

    // 决定返回值：如果子元素有更高分的，返回子元素；否则返回自身
    match best_child {
        Some((child_el, child_score)) if child_score * 1.1 > final_score => {
            (child_el, child_score)
        }
        _ => (node, final_score),
    }
}

/// 提取元素中的纯文本（保持段落结构）。
fn extract_element_text(element: &scraper::ElementRef) -> String {
    let mut result = String::new();
    let mut last_was_block = false;

    collect_text(element, &mut result, &mut last_was_block, false);

    // 合并连续空白
    let mut cleaned = String::with_capacity(result.len());
    let mut last_was_ws = false;
    let mut last_was_newline = false;

    for ch in result.chars() {
        if ch == '\n' {
            if !last_was_newline {
                cleaned.push('\n');
                last_was_newline = true;
            }
            last_was_ws = false;
        } else if ch.is_whitespace() {
            if !last_was_ws && !last_was_newline {
                cleaned.push(' ');
            }
            last_was_ws = true;
        } else {
            cleaned.push(ch);
            last_was_ws = false;
            last_was_newline = false;
        }
    }

    cleaned.trim().to_string()
}

fn collect_text(
    element: &scraper::ElementRef,
    output: &mut String,
    last_was_block: &mut bool,
    is_noise: bool,
) {
    let tag_name = element.value().name().to_lowercase();

    // 噪音标签：完全跳过
    if NOISE_TAGS.contains(&tag_name.as_str()) {
        return;
    }

    // 非正文标签：标记为噪音域
    if NON_CONTENT_TAGS.contains(&tag_name.as_str()) {
        // 继续处理子元素但标记为噪音
        for child in element.children() {
            if let Some(child_el) = scraper::ElementRef::wrap(child) {
                collect_text(&child_el, output, last_was_block, true);
            } else if let Some(text) = child.value().as_text() {
                if !is_noise {
                    output.push_str(&text.text);
                }
            }
        }
        return;
    }

    // 检查 class/id 噪音标识
    if !is_noise {
        let class_str = element
            .value()
            .attr("class")
            .unwrap_or("")
            .to_lowercase();
        let id_str = element.value().attr("id").unwrap_or("").to_lowercase();
        let combined = format!("{class_str} {id_str}");

        if BAD_CONTENT_PATTERNS.iter().any(|p| combined.contains(p)) {
            // 标记为噪音，跳过
            return;
        }
    }

    // 块级标签：换行
    let block_tags = [
        "p", "div", "article", "section", "h1", "h2", "h3", "h4", "h5", "h6",
        "li", "tr", "br", "hr", "blockquote", "pre", "table", "ul", "ol",
        "dl", "dt", "dd", "header", "footer", "nav", "main", "aside",
    ];

    if block_tags.contains(&tag_name.as_str()) && !output.is_empty() {
        if !*last_was_block {
            output.push('\n');
            *last_was_block = true;
        }
    }

    // 处理子元素
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                if !is_noise {
                    output.push_str(&text.text);
                    *last_was_block = false;
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = scraper::ElementRef::wrap(child) {
                    collect_text(&child_el, output, last_was_block, is_noise);
                }
            }
            _ => {}
        }
    }

    // 块级标签结束后再加换行
    if block_tags.contains(&tag_name.as_str()) {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        *last_was_block = true;
    }
}

/// 统计元素中链接内的文本长度。
fn count_link_text(element: &scraper::ElementRef) -> usize {
    let mut count = 0;
    let tag_name = element.value().name().to_lowercase();

    if tag_name == "a" {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                count += text.text.trim().len();
            }
            if let Some(child_el) = scraper::ElementRef::wrap(child) {
                count += count_link_text(&child_el);
            }
        }
        return count;
    }

    for child in element.children() {
        if let Some(child_el) = scraper::ElementRef::wrap(child) {
            count += count_link_text(&child_el);
        }
    }
    count
}

/// 策略 2: 使用已知的正文选择器提取。
fn extract_by_known_selectors(document: &Html) -> Option<String> {
    let selectors = [
        "article", "main", "[role=\"main\"]",
        ".article", ".post", ".content", ".entry",
        ".article-content", ".post-content", ".entry-content",
        ".main-content", "#content", "#article", "#main",
        ".detail-content", ".news-content", ".story-body",
    ];

    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&sel).next() {
                let text = extract_element_text(&el);
                if text.len() > 100 {
                    return Some(text);
                }
            }
        }
    }
    None
}

/// 策略 3: 全局 body 文本提取（降级方案）。
fn extract_body_text(document: &Html) -> String {
    if let Some(body) = document
        .select(&Selector::parse("body").unwrap())
        .next()
    {
        extract_element_text(&body)
    } else {
        String::new()
    }
}

/// 清理文本：去除噪音关键词行，保留有意义的内容。
fn clean_and_format(text: &str) -> String {
    let noise_lines: HashSet<&str> = [
        "javascript:", "cookie", "copyright", "©", "all rights reserved",
        "隐私政策", "隐私条款", "用户协议", "服务条款",
        "广告", "赞助", "推广",
        "上一篇", "下一篇", "相关阅读", "推荐阅读", "热门文章",
    ]
    .iter()
    .copied()
    .collect();

    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            if l.is_empty() {
                return false;
            }
            let lower = l.to_lowercase();
            if lower.len() < 4 && !lower.contains(|c: char| c.is_alphabetic()) {
                return false;
            }
            !noise_lines.iter().any(|nl| lower.contains(nl))
        })
        .collect();

    // 合并过短的行到前一行
    let mut result = Vec::new();
    for line in lines {
        if line.len() < 20 && !result.is_empty() {
            let last: &mut String = result.last_mut().unwrap();
            last.push(' ');
            last.push_str(line);
        } else {
            result.push(line.to_string());
        }
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_article_content() {
        let html = r##"<!DOCTYPE html>
<html>
<head><title>Test Article</title></head>
<body>
    <nav><ul><li><a href="/">Home</a></li></ul></nav>
    <header><h1>Site Header</h1></header>
    <article>
        <h1>Article Title</h1>
        <p>This is the first paragraph of the article. It contains meaningful content about the topic.</p>
        <p>This is the second paragraph with more details and information that readers would find useful.</p>
        <p>Conclusion paragraph wrapping up the discussion.</p>
    </article>
    <aside>
        <div class="sidebar">
            <h3>Related Articles</h3>
            <ul><li><a href="#">Link 1</a></li></ul>
        </div>
    </aside>
    <footer>
        <p>Copyright 2024. All rights reserved.</p>
    </footer>
    <script>console.log('test');</script>
</body>
</html>"##;

        let result = extract_main_content(html);
        assert!(result.contains("Title: Test Article"));
        assert!(result.contains("first paragraph"));
        assert!(result.contains("second paragraph"));
        assert!(!result.contains("Home"));
        assert!(!result.contains("Copyright"));
        assert!(!result.contains("console.log"));
    }

    #[test]
    fn test_extract_div_style_content() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>News Page</title></head>
<body>
    <div class="header">Header stuff</div>
    <div class="content">
        <div class="article-content">
            <h2>Breaking News</h2>
            <p>Something important happened today.</p>
            <p>The situation is developing rapidly.</p>
        </div>
    </div>
    <div class="footer">Footer links</div>
</body>
</html>"#;

        let result = extract_main_content(html);
        assert!(result.contains("Breaking News"));
        assert!(result.contains("important happened"));
        assert!(!result.contains("Header stuff"));
        assert!(!result.contains("Footer links"));
    }

    #[test]
    fn test_extract_fallback_body() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Simple Page</title></head>
<body>
    <p>Just a simple paragraph without any semantic structure.</p>
    <p>Another paragraph of text for testing purposes.</p>
</body>
</html>"#;

        let result = extract_main_content(html);
        assert!(result.contains("Simple Page"));
        assert!(result.contains("simple paragraph"));
    }
}
