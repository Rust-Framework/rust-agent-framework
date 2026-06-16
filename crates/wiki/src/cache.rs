use std::sync::{Arc, RwLock};

/// Simple generation-keyed cache. Stores a value alongside a generation key.
/// When the key changes, the cache is invalidated.
pub struct GenerationCache<T: Clone> {
    inner: RwLock<Option<(u64, T)>>,
}

impl<T: Clone> GenerationCache<T> {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Get the cached value if the generation key matches, otherwise call `builder` 
    /// to construct a new value and cache it.
    pub fn get_or_build(
        &self,
        current_gen: u64,
        builder: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<Arc<T>> {
        {
            let guard = self.inner.read().unwrap();
            if let Some((gen, val)) = guard.as_ref() {
                if *gen == current_gen {
                    return Ok(Arc::new(val.clone()));
                }
            }
        }
        // Build new value
        let new_val = builder()?;
        let result = Arc::new(new_val.clone());
        let mut guard = self.inner.write().unwrap();
        *guard = Some((current_gen, new_val));
        Ok(result)
    }

    /// Force invalidate the cache.
    pub fn invalidate(&self) {
        let mut guard = self.inner.write().unwrap();
        *guard = None;
    }
}

impl<T: Clone> Default for GenerationCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Wiki graph cache — in-memory only (no disk snapshots).
pub enum WikiGraphCache {
    InMemory(GenerationCache<crate::graph::WikiGraph>),
}

impl WikiGraphCache {
    pub fn new() -> Self {
        WikiGraphCache::InMemory(GenerationCache::new())
    }

    /// Return the current graph, rebuilding if the generation key changed.
    pub fn get_fresh(
        &self,
        current_gen: u64,
        builder: impl FnOnce() -> anyhow::Result<crate::graph::WikiGraph>,
    ) -> anyhow::Result<Arc<crate::graph::WikiGraph>> {
        match self {
            WikiGraphCache::InMemory(cache) => cache.get_or_build(current_gen, builder),
        }
    }

    /// Force a full rebuild.
    pub fn rebuild(
        &self,
        current_gen: u64,
        builder: impl FnOnce() -> anyhow::Result<crate::graph::WikiGraph>,
    ) -> anyhow::Result<Arc<crate::graph::WikiGraph>> {
        match self {
            WikiGraphCache::InMemory(cache) => {
                cache.invalidate();
                cache.get_or_build(current_gen, builder)
            }
        }
    }
}

impl Default for WikiGraphCache {
    fn default() -> Self {
        Self::new()
    }
}
