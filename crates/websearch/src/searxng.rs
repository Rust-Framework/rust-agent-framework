//! SearXNG 客户端 —— 搜索自建/公共 SearXNG 实例。
//!
//! SearXNG 是开源元搜索引擎，聚合 Google、Bing、DuckDuckGo 等 70+ 引擎。
//! 需自行部署实例（Docker），或使用公共实例。

use crate::error::SearchError;
use crate::types::{SearchConfig, SearchResult, SearchResults, SearchSource};
use serde::Deserialize;
use tracing::warn;

/// 公共 SearXNG 实例列表（需验证可用性）。
///
/// 注意：公共实例的可用性和隐私策略无法保证，
/// 生产环境建议自建实例。
const PUBLIC_INSTANCES: &[&str] = &[
    "https://searx.be",
    "https://search.sapti.me",
    "https://searx.tiekoetter.com",
];

/// SearXNG API 搜索结果。
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SearXNGResponse {
    query: String,
    number_of_results: Option<u64>,
    results: Vec<SearXNGResult>,
    answers: Option<Vec<SearXNGAnswer>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SearXNGResult {
    title: String,
    url: String,
    content: Option<String>,
    engine: Option<String>,
    score: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SearXNGAnswer {
    answer: Option<String>,
    url: Option<String>,
}

/// 通过 SearXNG 进行搜索。
///
/// 优先使用 `config.searxng_url` 中配置的自建实例，
/// 如果未配置，则尝试公共实例。
pub async fn search_searxng(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let instances: Vec<&str> = if let Some(ref url) = config.searxng_url {
        vec![url.as_str()]
    } else {
        PUBLIC_INSTANCES.to_vec()
    };

    let mut last_error = None;

    for instance_url in &instances {
        let url = format!("{instance_url}/search");

        match try_searxng_search(query, &url, config).await {
            Ok(results) => return Ok(results),
            Err(e) => {
                warn!("SearXNG instance {} failed: {}", instance_url, e);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or(SearchError::NoResults))
}

async fn try_searxng_search(
    query: &str,
    search_url: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = crate::anti_detection::build_client(config)?;

    let response = client
        .get(search_url)
        .query(&[
            ("q", query),
            ("format", "json"),
            ("categories", "general"),
            ("safesearch", "0"),
        ])
        .send()
        .await?;

    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(SearchError::HttpStatus {
            code: status.as_u16(),
            message: format!("SearXNG returned {status}"),
        });
    }

    let json: SearXNGResponse = response.json().await?;

    let results: Vec<SearchResult> = json
        .results
        .into_iter()
        .take(config.max_results)
        .enumerate()
        .map(|(i, r)| {
            let snippet = r
                .content
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect();

            SearchResult {
                title: r.title,
                url: r.url,
                snippet,
                source: SearchSource::SearXNG,
                rank: i + 1,
            }
        })
        .collect();

    // 如果有 answers，附加到结果末尾
    let mut all_results = results;
    if let Some(answers) = json.answers {
        for answer in answers {
            let snippet = answer.answer.unwrap_or_default();
            if !snippet.is_empty() && all_results.len() < config.max_results {
                all_results.push(SearchResult {
                    title: format!("Answer: {query}"),
                    url: answer.url.unwrap_or_default(),
                    snippet,
                    source: SearchSource::SearXNG,
                    rank: all_results.len() + 1,
                });
            }
        }
    }

    if all_results.is_empty() {
        return Err(SearchError::NoResults);
    }

    Ok(SearchResults {
        query: query.to_string(),
        results: all_results,
        source: SearchSource::SearXNG,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_searxng_response() {
        let json = r#"{
            "query": "rust",
            "number_of_results": 100,
            "results": [
                {
                    "title": "Rust Programming Language",
                    "url": "https://www.rust-lang.org/",
                    "content": "A language empowering everyone to build reliable and efficient software.",
                    "engine": "google",
                    "score": 0.95
                },
                {
                    "title": "Rust (programming language) - Wikipedia",
                    "url": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
                    "content": "Rust is a multi-paradigm, general-purpose programming language.",
                    "engine": "wikipedia",
                    "score": 0.9
                }
            ],
            "answers": []
        }"#;

        let resp: SearXNGResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.query, "rust");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].title, "Rust Programming Language");
    }
}
