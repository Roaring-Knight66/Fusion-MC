use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct Cache<K, V> {
    store: Arc<RwLock<HashMap<K, (V, Instant)>>>,
    _cleanup_handle: Option<JoinHandle<()>>,
}

impl<K, V> Cache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            _cleanup_handle: None,
        }
    }

    pub fn with_smart_cleanup() -> Self {
        let store: Arc<RwLock<HashMap<K, (V, Instant)>>> = Arc::new(RwLock::new(HashMap::new()));
        let store_clone = Arc::clone(&store);

        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;

                let now = Instant::now();

                // Two-phase cleanup: Read to identify, write to remove
                let expired_keys: Vec<K> = {
                    let map = store_clone.read().await;
                    map.iter()
                        .filter(|(_, (_, exp))| now >= *exp)
                        .map(|(k, _)| k.clone())
                        .collect()
                };

                let (removed, next_expiry) = if !expired_keys.is_empty() {
                    let mut map = store_clone.write().await;
                    let now = Instant::now(); // Re-check time

                    let before = map.len();
                    for key in &expired_keys {
                        if let Some((_, exp)) = map.get(key) {
                            if now >= *exp {
                                map.remove(key);
                            }
                        }
                    }
                    let after = map.len();

                    // Calculate next expiry AFTER cleanup
                    let next = map.values().map(|(_, expire_at)| *expire_at).min();

                    (before - after, next)
                } else {
                    // No expired entries, just get next expiry
                    let map = store_clone.read().await;
                    let next = map.values().map(|(_, expire_at)| *expire_at).min();
                    (0, next)
                };

                // Log AFTER releasing locks
                if removed > 0 {
                    lighty_core::trace_info!(removed = removed, "Cache cleaned expired entries");
                }

                // Adaptive sleep
                if let Some(next) = next_expiry {
                    let wait = next
                        .saturating_duration_since(Instant::now())
                        .max(Duration::from_secs(1)) // Min 1s
                        .min(Duration::from_secs(300)); // Max 5min
                    tokio::time::sleep(wait).await;
                } else {
                    // No entries, sleep max duration
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
            }
        });

        Self {
            store,
            _cleanup_handle: Some(handle),
        }
    }

    pub async fn insert_with_ttl(&self, key: K, value: V, ttl: Duration) {
        let mut store = self.store.write().await;
        let expire_at = Instant::now() + ttl;
        store.insert(key, (value, expire_at));
    }

    pub async fn get_with_ttl(&self, key: &K) -> Option<V> {
        // Fast path: read lock
        let store = self.store.read().await;

        if let Some((value, expire_at)) = store.get(key) {
            let now = Instant::now();
            if now < *expire_at {
                return Some(value.clone());
            }

            // Entry expired - need to remove it
            // Don't keep read lock while acquiring write lock
            drop(store);

            // Acquire write lock and re-check (double-check locking)
            let mut store = self.store.write().await;

            // Re-validate: another thread might have refreshed/removed it
            match store.get(key) {
                Some((value, expire_at)) => {
                    if Instant::now() < *expire_at {
                        // Another thread refreshed it
                        return Some(value.clone());
                    }
                    // Still expired, remove it
                    store.remove(key);
                }
                None => {
                    // Another thread already removed it
                }
            }
        }

        None
    }

    /// Get or compute with Result-returning closure
    /// If the closure returns an error, the error is propagated and nothing is cached
    pub async fn get_or_try_insert_with<F, Fut, E>(
        &self,
        key: K,
        ttl: Duration,
        f: F,
    ) -> Result<V, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<V, E>>,
    {
        if let Some(v) = self.get_with_ttl(&key).await {
            return Ok(v);
        }

        let value = f().await?;
        self.insert_with_ttl(key.clone(), value.clone(), ttl).await;
        Ok(value)
    }

    pub async fn clear(&self) {
        let mut store = self.store.write().await;
        store.clear();
    }

    pub async fn len(&self) -> usize {
        let store = self.store.read().await;
        store.len()
    }

    pub async fn is_empty(&self) -> bool {
        let store = self.store.read().await;
        store.is_empty()
    }
}

impl<K, V> Default for Cache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// REMOVED Clone implementation - use Arc<Cache> instead

impl<K, V> Drop for Cache<K, V> {
    fn drop(&mut self) {
        if let Some(handle) = self._cleanup_handle.take() {
            handle.abort();
        }
    }
}
