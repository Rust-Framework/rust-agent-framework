# rust-agent-rag

RAG (Retrieval-Augmented Generation) support for the Rust Agent Framework ecosystem — document loading, chunking, embedding, vector storage, and retrieval.

## Overview

A modular RAG pipeline library providing:

- **Document loading** — Load and extract text from `.txt`, `.md`, and `.html` files
- **Text chunking** — Recursive character splitting and semantic boundary chunking
- **Embedding** — Pluggable embedding model interface with a simple built-in implementation
- **Vector storage** — In-memory vector store with cosine, euclidean, and dot-product distance metrics
- **Retrieval** — Similarity search and MMR (Maximal Marginal Relevance) strategies

## Public API

### Document Loading

| Type / Function | Description |
|---|---|
| `DocumentLoader` | Loads documents from files or directories |
| `TextLoader` | Load plain text files (`.txt`, `.md`) |
| `HtmlLoader` | Load and extract text from HTML files |
| `load_document(path)` | Convenience function — auto-detect format and load |
| `Document` | Loaded document: `id`, `content`, `metadata` |
| `DocumentMeta` | Document metadata: `source_path`, `file_type`, `created_at` |
| `DocumentId` | Unique document identifier (UUID v4) |

```rust
use rust_agent_rag::{DocumentLoader, TextLoader, load_document};

let loader = DocumentLoader::new()
    .with_loader(TextLoader::new())
    .with_loader(HtmlLoader::new());

let docs = loader.load_directory("./knowledge-base/")?;
// or load a single file
let doc = load_document("./knowledge-base/article.md")?;
```

### Text Chunking

| Type | Description |
|---|---|
| `Chunker` | Main chunking orchestrator |
| `RecursiveCharacterChunker` | Splits by separators recursively (`\n\n` → `\n` → ` ` → char) |
| `SemanticChunker` | Splits at semantic boundaries (paragraphs, sentences) |
| `ChunkStrategy` | Enum: `RecursiveCharacter`, `Semantic` |
| `ChunkOverlapStrategy` | Enum: `None`, `Fixed(usize)`, `Percentage(f64)` |
| `DocumentChunk` | Chunked text: `id`, `content`, `document_id`, `chunk_index`, `metadata` |

```rust
use rust_agent_rag::{Chunker, RecursiveCharacterChunker, ChunkStrategy};

let chunker = Chunker::new(RecursiveCharacterChunker::new(512, 128)); // chunk_size, overlap
let chunks = chunker.chunk_document(&doc)?;
```

### Embedding

| Type / Function | Description |
|---|---|
| `IEmbeddingModel` | Trait for embedding models |
| `EmbeddingModel` | Default embedding model implementation |
| `simple_embedding_model()` | Create a simple heuristic embedding model (for testing/no external API) |
| `embed_chunks(model, chunks)` | Batch-embed a list of chunks |
| `Embedding` | Vector representation (`Vec<f32>` with dimension) |

```rust
use rust_agent_rag::{simple_embedding_model, embed_chunks};

let model = simple_embedding_model();
let embeddings = embed_chunks(&model, &chunks)?;
```

### Vector Store

| Type | Description |
|---|---|
| `IVectorStore` | Trait for vector stores |
| `VectorStore` | Generic vector store implementation |
| `InMemoryVectorStore` | In-memory vector store (default) |
| `DistanceMetric` | Enum: `Cosine`, `Euclidean`, `DotProduct` |
| `VectorStoreError` | Vector store error type |
| `IndexEntry` | An entry in the vector index |

```rust
use rust_agent_rag::{InMemoryVectorStore, DistanceMetric, IVectorStore};

let mut store = InMemoryVectorStore::new(DistanceMetric::Cosine);

for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
    store.add(chunk.id.clone(), embedding.clone(), chunk.metadata.clone())?;
}
```

### Retrieval

| Type | Description |
|---|---|
| `IRetriever` | Trait for retrieval strategies |
| `Retriever` | Generic retriever implementation |
| `SimilarityRetriever` | Similarity-based retrieval (cosine/euclidean/dot) |
| `RetrieverOptions` | Retrieval configuration: `top_k`, `min_score`, `strategy` |
| `RetrieverResult` | Retrieval result: `chunk_id`, `score`, `document_id`, `content`, `metadata` |
| `SearchResult` | Search result with chunk and score |
| `TextChunk` | Text chunk with content and metadata |

```rust
use rust_agent_rag::{Retriever, RetrieverOptions, SimilarityRetriever};

let retriever = Retriever::new(store, SimilarityRetriever::new());
let options = RetrieverOptions {
    top_k: 5,
    min_score: Some(0.5),
    ..Default::default()
};

let results = retriever.search(&query_embedding, &options)?;
for result in &results {
    println!("[score={:.2}] {}", result.score, result.content);
}
```

## Full Pipeline Example

```rust
use rust_agent_rag::*;

fn rag_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load documents
    let docs = DocumentLoader::new()
        .with_loader(TextLoader::new())
        .load_directory("./knowledge-base/")?;

    // 2. Chunk documents
    let chunker = Chunker::new(RecursiveCharacterChunker::new(512, 128));
    let chunks: Vec<_> = docs.iter()
        .flat_map(|doc| chunker.chunk_document(doc))
        .flatten()
        .collect();

    // 3. Embed chunks
    let model = simple_embedding_model();
    let embeddings = embed_chunks(&model, &chunks)?;

    // 4. Index into vector store
    let mut store = InMemoryVectorStore::new(DistanceMetric::Cosine);
    for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
        store.add(chunk.id.clone(), embedding.clone(), chunk.metadata.clone())?;
    }

    // 5. Query
    let query_embedding = model.embed("What is Rust ownership?")?;
    let retriever = Retriever::new(store, SimilarityRetriever::new());
    let results = retriever.search(&query_embedding, &RetrieverOptions::default())?;

    for r in &results {
        println!("[{}] {}", r.score, &r.content[..100.min(r.content.len())]);
    }

    Ok(())
}
```

## Features

Currently no feature flags. Future plans include optional `openai` / `ollama` embedding backends.

## Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime |
| `serde` / `serde_json` | Metadata and chunk serialization |
| `unicode-segmentation` | Unicode-aware text splitting for chunking |
| `uuid` | Document ID generation |
| `scraper` | HTML parsing for HtmlLoader |
| `thiserror` / `anyhow` | Error handling |
| `async-trait` | Async trait support |
| `tracing` | Structured logging |
