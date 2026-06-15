//! DuckDuckGo Instant Answer API 后端（`api.duckduckgo.com`）。
//!
//! 免费 JSON API，无需 API Key。适合知识类、定义类查询。
//! 返回 Abstract、RelatedTopics、Infobox 等结构化数据。

use crate::error::SearchError;
use crate::types::{SearchConfig, SearchResult, SearchResults, SearchSource};
use serde::Deserialize;

const INSTANT_ANSWER_URL: &str = "https://api.duckduckgo.com/";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct InstantAnswerResponse {
    #[serde(rename = "AbstractText")]
    abstract_text: String,
    #[serde(rename = "AbstractURL")]
    abstract_url: String,
    #[serde(rename = "AbstractSource")]
    abstract_source: String,
    #[serde(rename = "Heading")]
    heading: String,
    #[serde(rename = "Answer")]
    answer: String,
    #[serde(rename = "AnswerType")]
    answer_type: String,
    #[serde(rename = "Definition")]
    definition: String,
    #[serde(rename = "DefinitionSource")]
    definition_source: String,
    #[serde(rename = "RelatedTopics")]
    related_topics: Vec<RelatedTopic>,
    #[serde(rename = "Infobox")]
    infobox: Option<Infobox>,
    #[serde(rename = "Type")]
    response_type: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RelatedTopic {
    #[serde(rename = "Text")]
    text: String,
    #[serde(rename = "FirstURL")]
    first_url: String,
    #[serde(rename = "Result")]
    result: Option<String>,
    #[serde(rename = "Icon")]
    icon: Option<Icon>,
    #[serde(rename = "Topics")]
    #[serde(default)]
    topics: Vec<RelatedTopic>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Icon {
    #[serde(rename = "URL")]
    url: String,
}

#[derive(Debug, Deserialize)]
struct Infobox {
    #[serde(default)]
    content: Vec<InfoboxContent>,
    #[serde(default)]
    meta: Vec<InfoboxMeta>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct InfoboxContent {
    label: String,
    value: String,
    #[serde(rename = "data_type")]
    data_type: String,
}

#[derive(Debug, Deserialize)]
struct InfoboxMeta {
    label: String,
    value: String,
}

/// 通过 DuckDuckGo Instant Answer API 进行搜索。
pub async fn search_instant_answer(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = crate::anti_detection::build_client(config)?;

    let response = client
        .get(INSTANT_ANSWER_URL)
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
            ("t", "rust-agent-websearch"),
        ])
        .send()
        .await?;

    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(SearchError::HttpStatus {
            code: status.as_u16(),
            message: format!("DuckDuckGo Instant Answer returned {status}"),
        });
    }

    let json: InstantAnswerResponse = response.json().await?;

    let mut results: Vec<SearchResult> = Vec::new();
    let mut rank = 0;

    // 1. Abstract（摘要）
    if !json.abstract_text.is_empty() {
        rank += 1;
        let url = if !json.abstract_url.is_empty() {
            json.abstract_url.clone()
        } else {
            format!("https://duckduckgo.com/?q={}", urlencoding::encode(query))
        };
        let title = if !json.heading.is_empty() {
            json.heading.clone()
        } else {
            query.to_string()
        };

        results.push(SearchResult {
            title,
            url,
            snippet: clean_abstract(&json.abstract_text),
            source: SearchSource::DuckDuckGoInstantAnswer,
            rank,
        });
    }

    // 2. Answer（直接答案）
    if !json.answer.is_empty() && json.answer_type != "calc" {
        rank += 1;
        results.push(SearchResult {
            title: format!("Answer: {query}"),
            url: format!("https://duckduckgo.com/?q={}", urlencoding::encode(query)),
            snippet: clean_abstract(&json.answer),
            source: SearchSource::DuckDuckGoInstantAnswer,
            rank,
        });
    }

    // 3. Definition（定义）
    if !json.definition.is_empty() {
        rank += 1;
        let url = if !json.definition_source.is_empty() {
            json.definition_source.clone()
        } else {
            format!("https://duckduckgo.com/?q={}", urlencoding::encode(query))
        };
        results.push(SearchResult {
            title: format!("Definition: {query}"),
            url,
            snippet: clean_abstract(&json.definition),
            source: SearchSource::DuckDuckGoInstantAnswer,
            rank,
        });
    }

    // 4. RelatedTopics（相关主题）
    for topic in &json.related_topics {
        if results.len() >= config.max_results {
            break;
        }
        collect_topics(topic, &mut results, &mut rank, config.max_results);
    }

    // 5. Infobox
    if let Some(ref infobox) = json.infobox {
        for meta in &infobox.meta {
            if results.len() >= config.max_results {
                break;
            }
            rank += 1;
            results.push(SearchResult {
                title: meta.label.clone(),
                url: String::new(),
                snippet: meta.value.clone(),
                source: SearchSource::DuckDuckGoInstantAnswer,
                rank,
            });
        }
        for content in &infobox.content {
            if results.len() >= config.max_results {
                break;
            }
            rank += 1;
            results.push(SearchResult {
                title: content.label.clone(),
                url: String::new(),
                snippet: content.value.clone(),
                source: SearchSource::DuckDuckGoInstantAnswer,
                rank,
            });
        }
    }

    if results.is_empty() {
        return Err(SearchError::NoResults);
    }

    Ok(SearchResults {
        query: query.to_string(),
        results,
        source: SearchSource::DuckDuckGoInstantAnswer,
    })
}

fn collect_topics(
    topic: &RelatedTopic,
    results: &mut Vec<SearchResult>,
    rank: &mut usize,
    max: usize,
) {
    if results.len() >= max {
        return;
    }

    let text = topic.result.as_deref().unwrap_or(&topic.text);
    if !text.is_empty() && !topic.first_url.is_empty() {
        *rank += 1;
        // 提取第一句作为 title
        let title = text
            .split(&['.', '!', '?', '。', '？', '！'][..])
            .next()
            .unwrap_or(text)
            .to_string();

        results.push(SearchResult {
            title: clean_abstract(&title),
            url: topic.first_url.clone(),
            snippet: clean_abstract(text),
            source: SearchSource::DuckDuckGoInstantAnswer,
            rank: *rank,
        });
    }

    for sub in &topic.topics {
        if results.len() >= max {
            return;
        }
        collect_topics(sub, results, rank, max);
    }
}

/// 清理 Abstract 文本中的 HTML 标签和乱码。
fn clean_abstract(text: &str) -> String {
    // DuckDuckGo API 返回的文本可能包含 HTML 实体
    let cleaned = text
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<i>", "")
        .replace("</i>", "")
        .replace("<code>", "")
        .replace("</code>", "")
        .replace("<pre>", "")
        .replace("</pre>", "")
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ");
    crate::html_utils::decode_html_entities(&cleaned)
}

// 简单的 URL 编码（避免依赖 urlencoding crate）
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_instant_answer_json() {
        let json = r#"{
            "AbstractText": "Rust is a multi-paradigm programming language.",
            "AbstractURL": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "AbstractSource": "Wikipedia",
            "Heading": "Rust (programming language)",
            "Answer": "",
            "AnswerType": "",
            "Definition": "",
            "DefinitionSource": "",
            "RelatedTopics": [
                {
                    "Text": "Rust by Example",
                    "FirstURL": "https://doc.rust-lang.org/stable/rust-by-example/",
                    "Result": "<a href=\"https://doc.rust-lang.org/stable/rust-by-example/\">Rust by Example</a> - Learn Rust with examples.",
                    "Icon": {"URL": ""},
                    "Topics": []
                }
            ],
            "Infobox": null,
            "Type": "A"
        }"#;

        let resp: InstantAnswerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.abstract_text, "Rust is a multi-paradigm programming language.");
        assert_eq!(resp.heading, "Rust (programming language)");
        assert_eq!(resp.related_topics.len(), 1);
    }
}
