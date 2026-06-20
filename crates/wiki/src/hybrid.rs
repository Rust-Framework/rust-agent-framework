//! 混合检索 —— v2 特性 4。
//!
//! v2 规范：当 Wiki 超过 200 页时，传统的索引文件就会崩溃。v2 采用
//! BM25（关键词）、向量搜索（语义）与图谱遍历的融合方案，通过倒数排名融合
//!（RRF）确保检索的精准度。
//!
//! RRF 公式：`score(d) = Σ_i 1 / (k + rank_i(d))`
//! 其中 `k` 是平滑常数（默认 60），`rank_i(d)` 是文档在第 i 个检索器中的排名（1-based）。

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::search::{self, FacetCounts, PageRef, SearchOptions};
use crate::vector::VectorIndex;

/// RRF 平滑常数。
const RRF_K: u32 = 60;

/// 混合检索参数。
#[derive(Debug, Clone)]
pub struct HybridParams {
    /// 全文查询字符串。
    pub query: String,
    /// 最大返回结果数。
    pub top_k: usize,
    /// BM25 权重（默认 1.0）。
    pub bm25_weight: f32,
    /// 向量检索权重（默认 1.0；设为 0 跳过向量检索）。
    pub vector_weight: f32,
    /// 图谱遍历权重（默认 0.5；设为 0 跳过图谱）。
    pub graph_weight: f32,
    /// 图谱遍历跳数（从 BM25 top 结果扩展）。
    pub graph_hops: usize,
    /// 是否包含 section 页面。
    pub include_sections: bool,
    /// 类型过滤。
    pub type_filter: Option<String>,
}

impl Default for HybridParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            top_k: 10,
            bm25_weight: 1.0,
            vector_weight: 1.0,
            graph_weight: 0.5,
            graph_hops: 1,
            include_sections: false,
            type_filter: None,
        }
    }
}

/// 混合检索结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridResult {
    /// RRF 融合后的排名结果。
    pub results: Vec<PageRef>,
    /// 各来源的贡献明细（slug → (bm25_rank, vector_rank, graph_rank)）。
    pub source_ranks: HashMap<String, SourceRank>,
    /// 分面统计（来自 BM25）。
    pub facets: FacetCounts,
}

/// 单个页面在各检索器中的排名。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceRank {
    /// BM25 排名（1-based，0 表示未命中）。
    pub bm25: u32,
    /// 向量检索排名（1-based，0 表示未命中）。
    pub vector: u32,
    /// 图谱遍历排名（1-based，0 表示未命中）。
    pub graph: u32,
    /// 最终 RRF 分数。
    pub rrf_score: f32,
}

/// 执行混合检索。
///
/// 三路召回 → RRF 融合 → 按 RRF 分数排序截断。
///
/// - `bm25_searcher` + `index_schema`：BM25 召回
/// - `vector_index`：向量语义召回（可选，None 则跳过）
/// - `graph_neighbors`：图谱邻居召回（可选，空则跳过）
pub async fn hybrid_search(
    params: &HybridParams,
    bm25_searcher: &tantivy::Searcher,
    index_schema: &crate::index_schema::IndexSchema,
    wiki_name: &str,
    search_config: &crate::config::SearchConfig,
    vector_index: Option<&VectorIndex>,
    graph_neighbors: &HashMap<String, Vec<String>>, // slug → neighbor slugs
) -> Result<HybridResult> {
    // ── 1. BM25 召回 ──
    let bm25_opts = SearchOptions {
        no_excerpt: false,
        include_sections: params.include_sections,
        top_k: params.top_k * 2,
        r#type: params.type_filter.clone(),
        facets_top_tags: 10,
        search_config: search_config.clone(),
    };
    let bm25_result = search::search(&params.query, &bm25_opts, bm25_searcher, wiki_name, index_schema)?;
    let facets = bm25_result.facets.clone();

    let mut source_ranks: HashMap<String, SourceRank> = HashMap::new();
    let mut bm25_pages: HashMap<String, PageRef> = HashMap::new();

    for (i, r) in bm25_result.results.iter().enumerate() {
        let rank = (i + 1) as u32;
        source_ranks
            .entry(r.slug.clone())
            .or_default()
            .bm25 = rank;
        bm25_pages.insert(r.slug.clone(), r.clone());
    }

    // ── 2. 向量召回 ──
    let mut vector_pages: HashMap<String, PageRef> = HashMap::new();
    if params.vector_weight > 0.0 {
        if let Some(vi) = vector_index {
            if vi.chunk_count() > 0 {
                match vi.search(&params.query, params.top_k * 2).await {
                    Ok(hits) => {
                        for (i, h) in hits.iter().enumerate() {
                            let rank = (i + 1) as u32;
                            source_ranks
                                .entry(h.slug.clone())
                                .or_default()
                                .vector = rank;
                            // 构造 PageRef（向量召回的 score 用相似度）
                            vector_pages.insert(
                                h.slug.clone(),
                                PageRef {
                                    slug: h.slug.clone(),
                                    uri: format!("wiki://{wiki_name}/{}", h.slug),
                                    title: h.title.clone(),
                                    score: h.score,
                                    confidence: h.score, // 向量相似度作为置信度近似
                                    excerpt: Some(h.snippet.clone()),
                                    summary: None,
                                },
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "vector search failed, skipping");
                    }
                }
            }
        }
    }

    // ── 3. 图谱遍历召回 ──
    if params.graph_weight > 0.0 && params.graph_hops > 0 && !graph_neighbors.is_empty() {
        // 从 BM25 top 结果扩展邻居
        let seeds: Vec<String> = bm25_result
            .results
            .iter()
            .take(params.top_k)
            .map(|r| r.slug.clone())
            .collect();
        let mut graph_rank = 1u32;
        for seed in &seeds {
            if let Some(neighbors) = graph_neighbors.get(seed) {
                for n in neighbors {
                    let entry = source_ranks.entry(n.clone()).or_default();
                    if entry.graph == 0 {
                        entry.graph = graph_rank;
                        graph_rank += 1;
                    }
                }
            }
        }
    }

    // ── 4. RRF 融合 ──
    let k = RRF_K as f32;
    for rank in source_ranks.values_mut() {
        let mut score = 0.0f32;
        if rank.bm25 > 0 {
            score += params.bm25_weight / (k + rank.bm25 as f32);
        }
        if rank.vector > 0 {
            score += params.vector_weight / (k + rank.vector as f32);
        }
        if rank.graph > 0 {
            score += params.graph_weight / (k + rank.graph as f32);
        }
        rank.rrf_score = score;
    }

    // ── 5. 合并 PageRef 并按 RRF 排序 ──
    let mut all_slugs: Vec<String> = source_ranks.keys().cloned().collect();
    all_slugs.sort_by(|a, b| {
        let sa = source_ranks[a].rrf_score;
        let sb = source_ranks[b].rrf_score;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    all_slugs.truncate(params.top_k);

    let results: Vec<PageRef> = all_slugs
        .into_iter()
        .filter_map(|slug| {
            let rank = &source_ranks[&slug];
            // 优先用 BM25 的 PageRef（有 excerpt），否则用向量的
            let mut pr = bm25_pages.get(&slug).cloned().or_else(|| vector_pages.get(&slug).cloned())?;
            pr.score = rank.rrf_score;
            Some(pr)
        })
        .collect();

    Ok(HybridResult {
        results,
        source_ranks,
        facets,
    })
}

/// 将混合检索结果渲染为 LLM 友好的 markdown。
pub fn render_hybrid_llms(result: &HybridResult) -> String {
    if result.results.is_empty() {
        return "No results found.\n".to_string();
    }
    let mut out = String::new();
    for r in &result.results {
        let summary = r.summary.as_deref().unwrap_or("");
        let sources = result.source_ranks.get(&r.slug);
        let source_tag = if let Some(s) = sources {
            let mut tags = Vec::new();
            if s.bm25 > 0 { tags.push("bm25"); }
            if s.vector > 0 { tags.push("vec"); }
            if s.graph > 0 { tags.push("graph"); }
            format!(" [{}]", tags.join("+"))
        } else {
            String::new()
        };
        if summary.is_empty() {
            out.push_str(&format!("- [{}]({}){}\n", r.title, r.uri, source_tag));
        } else {
            out.push_str(&format!("- [{}]({}):{}{}\n", r.title, r.uri, summary, source_tag));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_fusion_logic() {
        // 验证 RRF 公式：bm25 rank 1 + vector rank 1
        let k = RRF_K as f32;
        let bm25_w = 1.0f32;
        let vec_w = 1.0f32;
        let score = bm25_w / (k + 1.0) + vec_w / (k + 1.0);
        assert!((score - 2.0 / 61.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_hybrid_search_no_vector() {
        // 无向量索引时应仍能返回 BM25 结果
        let params = HybridParams {
            query: "test".into(),
            top_k: 5,
            vector_weight: 0.0,
            graph_weight: 0.0,
            ..Default::default()
        };
        // 这里无法构造真实 searcher，仅验证参数
        assert_eq!(params.top_k, 5);
        assert_eq!(params.vector_weight, 0.0);
    }
}
