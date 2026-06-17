//! 内容后处理清洗器。
//!
//! 对 servo-fetch 提取的 Markdown/纯文本内容进行行级过滤、模板文本去除、页脚检测等处理，
//! 去除导航栏残留、页脚、侧边栏、广告等噪音内容。

use regex::Regex;
use std::sync::LazyLock;

// ── 预编译正则 ──

static BOILERPLATE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Cookie 同意 / 隐私提示
        Regex::new(r"(?i)\b(cookie|cookies|隐私|cookie\s*政策|cookie\s*声明|cookie\s*通知|We\s+use\s+cookies?)\b").unwrap(),
        // Newsletter / 邮件订阅
        Regex::new(r"(?i)\b(subscribe|newsletter|订阅|邮件订阅|\bSign\s+up\b)\b").unwrap(),
        // 社交分享
        Regex::new(r"(?i)\b(Share\s+(on|to)|分享到|Tweet|Pin\s+it)\b").unwrap(),
        // "相关文章" / "推荐阅读"
        Regex::new(r"(?i)\b(Related\s+(Articles?|Posts?|Stories?)|You\s+[Mm]ay\s+[Aa]lso\s+[Ll]ike|Recommended|相关文章|推荐阅读|猜你喜欢)\b").unwrap(),
        // 分页文本
        Regex::new(r"(?i)^\s*(Page\s+\d+|上一页|下一页|Previous|Next)\s*$").unwrap(),
        // 评论区计数占位
        Regex::new(r"(?i)^\s*(\d+\s*(comments?|条评论|回复|条回复))\s*$").unwrap(),
        // "加载更多" / "展开全文"
        Regex::new(r"(?i)\b(Load\s+[Mm]ore|加载更多|展开全文|Read\s+[Mm]ore|Show\s+[Mm]ore)\b").unwrap(),
        // 广告标识
        Regex::new(r"(?i)\b((Sponsored|Advertisement|Promoted|广告|推广|赞助))\b").unwrap(),
        // 设置/偏好选项残留
        Regex::new(r"(?i)^\s*(Settings|Preferences|Dark\s+[Mm]ode|Language|Theme|设置|偏好|语言|主题)\s*$").unwrap(),
    ]
});

static FOOTER_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)(?:^|\b)([©©]\s*\d{4}|Copyright|All\s+[Rr]ights?\s+[Rr]eserved|版权所有)\b").unwrap(),
        Regex::new(r"(?i)\b(Terms\s+of\s+(Service|Use)|Privacy\s+Policy|Cookie\s+Policy|服务条款|隐私政策|使用协议)\b").unwrap(),
        Regex::new(r"(?i)\b(Powered\s+by|Built\s+with|Made\s+with|由.*提供|技术支持)\b").unwrap(),
        Regex::new(r"(?i)\b(Contact\s+[Uu]s|About\s+[Uu]s|联系我们|关于我们)\b").unwrap(),
        Regex::new(r"(?i)^\s*(Follow\s+us|关注我们|Social\s+Media|社交媒体)\s*$").unwrap(),
        Regex::new(r"(?i)\b(Sitemap|站点地图|RSS|Feed)\b").unwrap(),
        Regex::new(r"(?i)\b(Back\s+to\s+top|返回顶部)\b").unwrap(),
    ]
});

static SENTENCE_ENDING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[。！？.!?;；]$").unwrap()
});

// ── 行结构 ──

/// 对单行文本的分析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineCategory {
    /// 正文行——长度足够且包含自然语言特征
    Content,
    /// 模板行——匹配已知的模板文本模式
    Boilerplate,
    /// 可疑短行——过短，可能是导航条目
    SuspiciousShort,
    /// 空行/空白行
    Empty,
}

// ── 清洗器 ──

/// 内容清洗器，提供多层次的文本后处理能力。
pub struct ContentCleaner {
    /// 清洗模式
    mode: CleanMode,
}

impl ContentCleaner {
    /// 使用指定模式创建清洗器。
    pub fn new(mode: CleanMode) -> Self {
        Self { mode }
    }

    /// 清洗内容，返回清洗后的文本。
    pub fn clean(&self, text: &str) -> String {
        match self.mode {
            CleanMode::Raw => text.to_string(),
            CleanMode::Minimal => self.clean_minimal(text),
            CleanMode::Auto => self.clean_auto(text),
            CleanMode::Aggressive => self.clean_aggressive(text),
        }
    }

    /// 最小清洗：仅合并多余空白和空行。
    fn clean_minimal(&self, text: &str) -> String {
        normalize_whitespace(text)
    }

    /// 自动清洗：检测内容质量，动态选择策略。
    fn clean_auto(&self, text: &str) -> String {
        let cleaned = self.clean_minimal(text);
        let noise_ratio = compute_noise_ratio(&cleaned);

        if noise_ratio > 0.5 {
            // 噪音较多，采用激进策略
            self.clean_aggressive(text)
        } else if noise_ratio > 0.25 {
            // 中等噪音，温和清洗
            self.clean_standard(text)
        } else {
            // 低噪音，最小清洗即可
            cleaned
        }
    }

    /// 标准清洗：行级过滤 + 页脚检测 + 模板文本去除。
    fn clean_standard(&self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let categories: Vec<LineCategory> = lines.iter().map(|l| categorize_line(l)).collect();
        let footer_start = detect_footer_boundary(&lines, &categories);

        let end = footer_start.unwrap_or(lines.len());

        let filtered: Vec<&str> = lines[..end]
            .iter()
            .enumerate()
            .filter(|(i, _line)| {
                let cat = categories[*i];
                match cat {
                    LineCategory::Empty => false,
                    LineCategory::Boilerplate => false,
                    LineCategory::SuspiciousShort => {
                        // 仅当上下文中没有正文时才保留（独立短行可能是标题）
                        let has_content_neighbor = (*i > 0 && categories[i - 1] == LineCategory::Content)
                            || (i + 1 < categories.len() && categories[i + 1] == LineCategory::Content);
                        !has_content_neighbor
                    }
                    LineCategory::Content => true,
                }
            })
            .map(|(_, &line)| line)
            .collect();

        normalize_whitespace(&filtered.join("\n"))
    }

    /// 激进清洗：高阈值过滤 + 标记线去除。
    fn clean_aggressive(&self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let categories: Vec<LineCategory> = lines.iter().map(|l| categorize_line(l)).collect();
        let footer_start = detect_footer_boundary(&lines, &categories);

        let end = footer_start.unwrap_or(lines.len());

        let filtered: Vec<&str> = lines[..end]
            .iter()
            .enumerate()
            .filter(|(i, _line)| {
                let cat = categories[*i];
                // 激进模式：只保留正文，去掉所有短行
                match cat {
                    LineCategory::Content => true,
                    _ => false,
                }
            })
            .map(|(_, &line)| line)
            .collect();

        normalize_whitespace(&filtered.join("\n"))
    }
}

impl Default for ContentCleaner {
    fn default() -> Self {
        Self::new(CleanMode::Auto)
    }
}

// ── 清洗模式 ──

/// 内容清洗模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanMode {
    /// 自动选择策略（默认）
    Auto,
    /// 激进清洗——仅保留明确的正文行
    Aggressive,
    /// 最小清洗——仅合并空白
    Minimal,
    /// 原始输出——不做任何处理
    Raw,
}

impl CleanMode {
    /// 从字符串解析清洗模式。
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "aggressive" => Some(Self::Aggressive),
            "minimal" => Some(Self::Minimal),
            "raw" => Some(Self::Raw),
            _ => None,
        }
    }
}

// ── 行分类 ──

/// 对单行文本进行分类。
fn categorize_line(line: &str) -> LineCategory {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return LineCategory::Empty;
    }

    // 匹配模板模式
    if BOILERPLATE_PATTERNS.iter().any(|re| re.is_match(trimmed)) {
        return LineCategory::Boilerplate;
    }

    // 非常短的行（<= 10 字符）：可能是导航条目
    let char_count = trimmed.chars().count();
    if char_count <= 10 {
        return LineCategory::SuspiciousShort;
    }

    // 中短行（11-25 字符）：如果以标点结尾且包含中文字符，可能是短句；否则可疑
    if char_count <= 25 {
        let has_chinese = trimmed.chars().any(|c| c as u32 >= 0x4E00 && c as u32 <= 0x9FFF);
        let ends_with_punct = SENTENCE_ENDING.is_match(trimmed);
        if has_chinese && ends_with_punct {
            return LineCategory::Content;
        }
        return LineCategory::SuspiciousShort;
    }

    LineCategory::Content
}

// ── 页脚检测 ──

/// 检测页脚起始行索引。
///
/// 从后往前扫描，寻找第一批连续匹配页脚模式的行，返回最早匹配行的索引。
fn detect_footer_boundary(lines: &[&str], categories: &[LineCategory]) -> Option<usize> {
    let total = lines.len();
    if total < 5 {
        return None;
    }

    // 只检查最后 30% 或最后 15 行（取较大值）
    let check_window = (total / 3).max(15).min(total);
    let start_check = total - check_window;

    // 从后往前扫描，寻找连续的页脚行
    let mut footer_lines_seen = 0;
    let mut _footer_end = None;

    for i in (start_check..total).rev() {
        let trimmed = lines[i].trim();
        let is_footer = FOOTER_PATTERNS.iter().any(|re| re.is_match(trimmed))
            || (categories[i] == LineCategory::Boilerplate);

        if is_footer {
            footer_lines_seen += 1;
            _footer_end = Some(i);
        } else if footer_lines_seen > 0 {
            // 遇到一个非页脚行，如果页脚行数量 >= 2 且不在段落中间，则确认页脚
            if footer_lines_seen >= 2 {
                // 确保该行之后不会又出现正文（避免误判）
                let has_content_after = lines[i + 1..]
                    .iter()
                    .any(|l| {
                        let trimmed = l.trim();
                        !trimmed.is_empty()
                            && !FOOTER_PATTERNS.iter().any(|re| re.is_match(trimmed))
                            && categorize_line(trimmed) == LineCategory::Content
                    });
                if !has_content_after {
                    return Some(i + 1);
                }
            }
            footer_lines_seen = 0;
            _footer_end = None;
        }
    }

    None
}

// ── 噪声比计算 ──

/// 计算内容的噪声比例（0.0 ~ 1.0）。
///
/// 噪声比 = 模板行数 / 总行数（忽略空行）。
fn compute_noise_ratio(text: &str) -> f64 {
    let lines: Vec<&str> = text.lines().collect();
    let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();

    if non_empty.is_empty() {
        return 0.0;
    }

    let boilerplate_count = non_empty
        .iter()
        .filter(|l| categorize_line(l.trim()) == LineCategory::Boilerplate)
        .count();

    boilerplate_count as f64 / non_empty.len() as f64
}

// ── 空白规范化 ──

/// 规范化空白：合并连续空行、合并行内多余空格、去除首尾空白。
pub fn normalize_whitespace(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank && !result.is_empty() {
                result.push(String::new());
                prev_blank = true;
            }
        } else {
            // 合并行内多余空白
            let compacted = collapse_spaces(trimmed);
            result.push(compacted);
            prev_blank = false;
        }
    }

    // 去除末尾空行
    while result.last().map_or(false, |s| s.is_empty()) {
        result.pop();
    }

    result.join("\n")
}

/// 合并行内的连续空白字符为单个空格。
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

// ── 公开的质量评分函数 ──

/// 对文本内容进行质量评分（0.0 ~ 1.0）。
///
/// 评分维度：
/// - 信号比：正文行占比
/// - 句子完整性：以句末标点结尾的行占比
/// - 链接密度惩罚：URL 密度过高时扣分
/// - 重复惩罚：重复短行过多时扣分
/// - 结构加分：句子长度分布接近自然文本时加分
pub fn score_content(text: &str) -> f64 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return 0.0;
    }

    let total = lines.len() as f64;

    // ── 1. 信号比 ──
    let content_lines = lines
        .iter()
        .filter(|l| categorize_line(l.trim()) == LineCategory::Content)
        .count() as f64;
    let signal_ratio = content_lines / total;

    // ── 2. 句子完整性 ──
    let complete_sentences = lines
        .iter()
        .filter(|l| SENTENCE_ENDING.is_match(l.trim()))
        .count() as f64;
    let completeness = complete_sentences / total;

    // ── 3. 链接密度惩罚 ──
    let link_density = compute_link_density(&lines);
    let link_penalty = (link_density * 2.0).min(1.0); // 链接密度越高惩罚越重

    // ── 4. 重复惩罚 ──
    let repetition_rate = compute_repetition_rate(&lines);
    let repetition_penalty = (repetition_rate * 1.5).min(1.0);

    // ── 5. 结构加分 ──
    let structure_bonus = compute_structure_bonus(&lines);

    // 综合评分：信号比 40% + 完整性 20% - 链接惩罚 20% - 重复惩罚 15% + 结构加分 5%
    let score = signal_ratio * 0.40
        + completeness * 0.20
        - link_penalty * 0.20
        - repetition_penalty * 0.15
        + structure_bonus * 0.05;

    score.clamp(0.0, 1.0)
}

/// 计算文本中的链接密度（每行的平均 URL 数量）
fn compute_link_density(lines: &[&str]) -> f64 {
    let url_re = Regex::new(r"https?://\S+").unwrap();
    let total_urls: usize = lines.iter().map(|l| url_re.find_iter(l).count()).sum();
    let density = total_urls as f64 / lines.len().max(1) as f64;
    density.min(1.0)
}

/// 计算重复率——检测重复出现的短行。
fn compute_repetition_rate(lines: &[&str]) -> f64 {
    if lines.len() < 3 {
        return 0.0;
    }

    let short_lines: Vec<&str> = lines
        .iter()
        .filter(|l| l.trim().chars().count() <= 30)
        .copied()
        .collect();

    if short_lines.is_empty() {
        return 0.0;
    }

    // 使用简单的出现次数统计来估算重复率
    let total_short = short_lines.len() as f64;
    let mut unique_count = 0usize;
    let mut seen = std::collections::HashSet::new();

    for line in &short_lines {
        if seen.insert(line.trim().to_lowercase()) {
            unique_count += 1;
        }
    }

    let uniqueness = unique_count as f64 / total_short.max(1.0);
    (1.0 - uniqueness).min(1.0)
}

/// 计算结构加分——句子长度分布更接近自然文本时加分。
fn compute_structure_bonus(lines: &[&str]) -> f64 {
    let content_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| categorize_line(l.trim()) == LineCategory::Content)
        .collect();

    if content_lines.len() < 3 {
        return 0.0;
    }

    let lengths: Vec<usize> = content_lines
        .iter()
        .map(|l| l.trim().chars().count())
        .collect();

    let mean = lengths.iter().sum::<usize>() as f64 / lengths.len() as f64;

    let variance = lengths
        .iter()
        .map(|&l| {
            let diff = l as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / lengths.len() as f64;

    let std_dev = variance.sqrt();

    // 中等标准差（20-80）说明句子长度有一定变化，接近自然文本
    // 标准差太小说明都是短行（可能是列表），太大说明格式混乱
    if (20.0..=120.0).contains(&std_dev) {
        // 在理想范围内线性映射到 0.0-1.0
        let ideal = 60.0; // 理想的标准差
        let distance = (std_dev - ideal).abs();
        (1.0 - distance / 80.0).max(0.0)
    } else if std_dev < 20.0 {
        std_dev / 20.0 * 0.3 // 太小，部分加分
    } else {
        (1.0 - (std_dev - 120.0) / 200.0).max(0.0) * 0.5 // 太大，部分加分
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── categorize_line 测试 ──

    #[test]
    fn test_categorize_content_line() {
        assert_eq!(
            categorize_line("这是一段正常的正文内容，包含足够的字符数以通过分类检测。"),
            LineCategory::Content
        );
        assert_eq!(
            categorize_line("This is a normal content line with enough characters to be considered text."),
            LineCategory::Content
        );
    }

    #[test]
    fn test_categorize_short_line() {
        assert_eq!(categorize_line("导航"), LineCategory::SuspiciousShort);
        assert_eq!(categorize_line("Home"), LineCategory::SuspiciousShort);
    }

    #[test]
    fn test_categorize_empty_line() {
        assert_eq!(categorize_line(""), LineCategory::Empty);
        assert_eq!(categorize_line("   "), LineCategory::Empty);
    }

    #[test]
    fn test_categorize_boilerplate() {
        assert_eq!(
            categorize_line("We use cookies to improve your experience"),
            LineCategory::Boilerplate
        );
        assert_eq!(
            categorize_line("Subscribe to our newsletter"),
            LineCategory::Boilerplate
        );
        assert_eq!(categorize_line("Share on Twitter"), LineCategory::Boilerplate);
        assert_eq!(categorize_line("相关文章"), LineCategory::Boilerplate);
    }

    // ── 页脚检测测试 ──

    #[test]
    fn test_footer_regex_match() {
        // 验证页脚正则能匹配典型页脚文本
        let copyright_re = &FOOTER_PATTERNS[0]; // Copyright 模式
        assert!(copyright_re.is_match("Copyright 2024 Example Corp"));
        assert!(copyright_re.is_match("© 2024 Acme Inc."));

        let terms_re = &FOOTER_PATTERNS[1]; // 服务条款模式
        assert!(terms_re.is_match("Terms of Service"));
        assert!(terms_re.is_match("Privacy Policy"));
        assert!(terms_re.is_match("隐私政策"));

        let powered_re = &FOOTER_PATTERNS[2]; // Powered by 模式
        assert!(powered_re.is_match("Powered by WordPress"));
    }

    #[test]
    fn test_detect_footer() {
        let lines = vec![
            "这是第一段正文内容，包含足够多的字符使得分类器识别为正文。",
            "第二段正文内容，同样有足够的字符数量。",
            "这是第三段正文，文章的主要内容在这里。",
            "这里还有一些补充说明信息。",
            "继续正文内容，确保前面有足够的正文行。",
            "Copyright 2024 Example Corp",
            "All rights reserved",
            "Privacy Policy | Terms of Service",
            "Powered by WordPress",
        ];
        let categories: Vec<LineCategory> = lines.iter().map(|l| categorize_line(l)).collect();
        let result = detect_footer_boundary(&lines, &categories);
        assert_eq!(result, Some(5)); // 页脚从第6行 "Copyright" 开始
    }

    #[test]
    fn test_no_footer_in_short_text() {
        let lines = vec!["Short content", "Copyright 2024"];
        let categories: Vec<LineCategory> = lines.iter().map(|l| categorize_line(l)).collect();
        assert_eq!(detect_footer_boundary(&lines, &categories), None);
    }

    // ── 清洗模式测试 ──

    #[test]
    fn test_clean_raw() {
        let cleaner = ContentCleaner::new(CleanMode::Raw);
        let input = "  hello   world  ";
        assert_eq!(cleaner.clean(input), "  hello   world  ");
    }

    #[test]
    fn test_clean_minimal() {
        let cleaner = ContentCleaner::new(CleanMode::Minimal);
        let input = "  hello   world  \n\n\nfoo";
        assert_eq!(cleaner.clean(input), "hello world\n\nfoo");
    }

    #[test]
    fn test_clean_standard_removes_boilerplate() {
        let cleaner = ContentCleaner::new(CleanMode::Auto);
        let input = "This is real content.\nSubscribe to our newsletter\nMore real content.\nShare on Twitter";
        let result = cleaner.clean(input);
        assert!(result.contains("real content"));
        assert!(!result.contains("Subscribe"));
        assert!(!result.contains("Share on Twitter"));
    }

    #[test]
    fn test_clean_aggressive() {
        let cleaner = ContentCleaner::new(CleanMode::Aggressive);
        let input = "Home\nAbout\n这是一段真正的正文内容，包含足够的信息。\nContact\n另一段有意义的正文文本。";
        let result = cleaner.clean(input);
        assert!(result.contains("这是一段真正的正文内容"));
        assert!(result.contains("另一段有意义的正文文本"));
        assert!(!result.contains("Home"));
        assert!(!result.contains("Contact"));
    }

    // ── 质量评分测试 ──

    #[test]
    fn test_score_good_content() {
        let text = "这是一段质量很高的正文内容。\n包含多个完整的句子。\n每个句子都有明确的句末标点。\n文章结构清晰，内容充实。";
        let score = score_content(text);
        assert!(score > 0.5, "Good content should score high, got {}", score);
    }

    #[test]
    fn test_score_noisy_content() {
        let text = "Home\nAbout\nContact\nShare on Twitter\nSubscribe\nFollow us\nCopyright 2024\nBack to top";
        let score = score_content(text);
        assert!(score < 0.5, "Noisy content should score low, got {}", score);
    }

    // ── CleanMode 解析测试 ──

    #[test]
    fn test_clean_mode_from_str() {
        assert_eq!(CleanMode::from_str("auto"), Some(CleanMode::Auto));
        assert_eq!(CleanMode::from_str("aggressive"), Some(CleanMode::Aggressive));
        assert_eq!(CleanMode::from_str("minimal"), Some(CleanMode::Minimal));
        assert_eq!(CleanMode::from_str("raw"), Some(CleanMode::Raw));
        assert_eq!(CleanMode::from_str("AUTO"), Some(CleanMode::Auto));
        assert_eq!(CleanMode::from_str("unknown"), None);
    }

    // ── 空白规范化测试 ──

    #[test]
    fn test_normalize_whitespace_merges_blank_lines() {
        let input = "line1\n\n\n\nline2\n\nline3";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line1\n\nline2\n\nline3");
    }

    #[test]
    fn test_normalize_whitespace_trims() {
        let input = "\n\n  line1  \n  line2  \n\n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line1\nline2");
    }
}
