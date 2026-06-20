use std::collections::HashMap;

use anyhow::Result;

use crate::engine::EngineState;
use crate::search;

/// Parameters for the `search` operation.
#[derive(Default)]
pub struct SearchParams<'a> {
    /// Full-text query string.
    pub query: &'a str,
    /// Restrict results to this frontmatter type.
    pub type_filter: Option<&'a str>,
    /// When true, omit body excerpts from results.
    pub no_excerpt: bool,
    /// Maximum number of results to return.
    pub top_k: Option<usize>,
    /// When true, include section index pages in results.
    pub include_sections: bool,
    /// When true, search across all mounted wikis.
    pub cross_wiki: bool,
    /// v2: When true, use hybrid search (BM25 + vector + graph, RRF fused).
    pub hybrid: bool,
    /// v2: Vector search weight (default 1.0; only used when hybrid=true).
    pub vector_weight: Option<f32>,
    /// v2: Graph traversal weight (default 0.5; only used when hybrid=true).
    pub graph_weight: Option<f32>,
    /// v2: Graph traversal hops (default 1; only used when hybrid=true).
    pub graph_hops: Option<usize>,
}

/// Run a search against the wiki index (BM25 or hybrid).
pub fn search(
    engine: &EngineState,
    wiki_name: &str,
    params: &SearchParams<'_>,
) -> Result<search::SearchResult> {
    if params.hybrid {
        return search_hybrid_sync(engine, wiki_name, params);
    }
    search_bm25(engine, wiki_name, params)
}

/// Run a BM25 search against the wiki index.
fn search_bm25(
    engine: &EngineState,
    wiki_name: &str,
    params: &SearchParams<'_>,
) -> Result<search::SearchResult> {
    let space = engine.space(wiki_name)?;
    let resolved = space.resolved_config(&engine.config);

    let opts = search::SearchOptions {
        no_excerpt: params.no_excerpt,
        include_sections: params.include_sections,
        top_k: params
            .top_k
            .unwrap_or(resolved.defaults.search_top_k as usize),
        r#type: params.type_filter.map(|s| s.to_string()),
        facets_top_tags: resolved.defaults.facets_top_tags as usize,
        search_config: resolved.search.clone(),
    };

    if params.cross_wiki {
        let mut wikis = Vec::new();
        for s in engine.spaces.values() {
            let searcher = s.index_manager.searcher()?;
            wikis.push((s.name.clone(), searcher, &s.index_schema));
        }
        return search::search_all(params.query, &opts, &wikis);
    }

    let searcher = space.index_manager.searcher()?;
    search::search(
        params.query,
        &opts,
        &searcher,
        wiki_name,
        &space.index_schema,
    )
}

/// v2: Run a hybrid search (BM25 + vector + graph, RRF fused).
///
/// Note: This is a synchronous wrapper that blocks on the async hybrid_search.
/// For async contexts, use `search_hybrid_async` directly.
fn search_hybrid_sync(
    engine: &EngineState,
    wiki_name: &str,
    params: &SearchParams<'_>,
) -> Result<search::SearchResult> {
    let rt = tokio::runtime::Handle::try_current()
        .map_err(|e| anyhow::anyhow!("hybrid search requires a tokio runtime: {e}"))?;
    rt.block_on(search_hybrid_async(engine, wiki_name, params))
}

/// v2: Async hybrid search.
pub async fn search_hybrid_async(
    engine: &EngineState,
    wiki_name: &str,
    params: &SearchParams<'_>,
) -> Result<search::SearchResult> {
    let space = engine.space(wiki_name)?;
    let resolved = space.resolved_config(&engine.config);
    let top_k = params
        .top_k
        .unwrap_or(resolved.defaults.search_top_k as usize);

    let hybrid_params = crate::hybrid::HybridParams {
        query: params.query.to_string(),
        top_k,
        bm25_weight: 1.0,
        vector_weight: params.vector_weight.unwrap_or(1.0),
        graph_weight: params.graph_weight.unwrap_or(0.5),
        graph_hops: params.graph_hops.unwrap_or(1),
        include_sections: params.include_sections,
        type_filter: params.type_filter.map(|s| s.to_string()),
    };

    let searcher = space.index_manager.searcher()?;
    let vi = space.vector_index.read().clone();

    // 构建图谱邻居映射（从 graph cache）
    let graph_neighbors: HashMap<String, Vec<String>> = if hybrid_params.graph_weight > 0.0 {
        build_graph_neighbors(space)
    } else {
        HashMap::new()
    };

    let result = crate::hybrid::hybrid_search(
        &hybrid_params,
        &searcher,
        &space.index_schema,
        wiki_name,
        &resolved.search,
        vi.as_deref(),
        &graph_neighbors,
    )
    .await?;

    // 转换 HybridResult → SearchResult
    Ok(search::SearchResult {
        results: result.results,
        facets: result.facets,
    })
}

/// 从 graph cache 构建 slug → neighbor slugs 映射。
fn build_graph_neighbors(space: &crate::engine::SpaceContext) -> HashMap<String, Vec<String>> {
    let mut neighbors = HashMap::new();
    // 直接构建图（绕过 cache 的 generation 机制，用于一次性邻居查询）
    let graph_result = {
        let searcher = match space.index_manager.searcher() {
            Ok(s) => s,
            Err(_) => return neighbors,
        };
        crate::graph::build_graph(
            &searcher,
            &space.index_schema,
            &crate::graph::GraphFilter::default(),
            &space.type_registry,
        )
    };
    if let Ok(graph) = graph_result {
        for edge_ref in graph.edge_indices() {
            if let Some((src, dst)) = graph.edge_endpoints(edge_ref) {
                let src_slug = graph[src].slug.clone();
                let dst_slug = graph[dst].slug.clone();
                neighbors
                    .entry(src_slug)
                    .or_insert_with(Vec::new)
                    .push(dst_slug);
            }
        }
    }
    neighbors
}

/// Return a paginated listing of wiki pages with optional type/status filters.
pub fn list(
    engine: &EngineState,
    wiki_name: &str,
    type_filter: Option<&str>,
    status: Option<&str>,
    page: usize,
    page_size: Option<usize>,
) -> Result<search::PageList> {
    let space = engine.space(wiki_name)?;
    let resolved = space.resolved_config(&engine.config);

    let opts = search::ListOptions {
        r#type: type_filter.map(|s| s.to_string()),
        status: status.map(|s| s.to_string()),
        page,
        page_size: page_size.unwrap_or(resolved.defaults.list_page_size as usize),
        facets_top_tags: resolved.defaults.facets_top_tags as usize,
    };
    let searcher = space.index_manager.searcher()?;
    search::list(&opts, &searcher, wiki_name, &space.index_schema)
}
