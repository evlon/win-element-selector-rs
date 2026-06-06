// src/core/element_cache.rs
//
// Thread-safe element cache for find-from-element API and runtimeId-based operations.
// Uses uiautomation-rs UIElement type (Send + Sync under MTA).
//
// Optimizations:
// - VecDeque instead of Vec for O(1) popleft eviction
// - True LRU: get() hit promotes key to VecDeque tail
// - Graceful lock poisoning recovery
// - TTL support: auto-expire cached entries after configurable duration

use std::collections::{HashMap, VecDeque};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};
use uiautomation::core::UIElement;

/// Maximum number of cached elements before eviction.
const MAX_CACHE_SIZE: usize = 512;

/// A cached element with its timestamp.
struct CachedElement {
    element: UIElement,
    cached_at: Instant,
}

struct ElementCache {
    /// RuntimeId string → cached entry.
    elements: HashMap<String, CachedElement>,
    /// Insertion order for LRU eviction (VecDeque for O(1) pop_front).
    insertion_order: VecDeque<String>,
    /// Global default TTL. None = never expires.
    default_ttl: Option<Duration>,
}

impl ElementCache {
    fn new() -> Self {
        Self {
            elements: HashMap::new(),
            insertion_order: VecDeque::new(),
            default_ttl: None,
        }
    }

    fn insert(&mut self, key: String, element: UIElement) {
        if self.elements.contains_key(&key) {
            return; // Already cached
        }

        // Evict oldest entries if at capacity
        while self.elements.len() >= MAX_CACHE_SIZE {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.elements.remove(&oldest);
            } else {
                break;
            }
        }

        self.elements.insert(key.clone(), CachedElement {
            element,
            cached_at: Instant::now(),
        });
        self.insertion_order.push_back(key);
    }

    /// Get cached element, checking TTL expiry. None = expired or not found.
    fn get(&mut self, key: &str) -> Option<UIElement> {
        self.get_with_ttl(key, self.default_ttl)
    }

    /// Get cached element with a custom TTL (overrides global default).
    /// `ttl` = None means never expire (global default may still apply).
    fn get_with_ttl(&mut self, key: &str, ttl: Option<Duration>) -> Option<UIElement> {
        if let Some(entry) = self.elements.get(key) {
            // Check expiry
            let effective_ttl = ttl.or(self.default_ttl);
            if let Some(ttl_dur) = effective_ttl {
                if entry.cached_at.elapsed() > ttl_dur {
                    // Expired → remove and return None
                    self.elements.remove(key);
                    self.insertion_order.retain(|k| k != key);
                    return None;
                }
            }
            // Valid → LRU promote
            self.insertion_order.retain(|k| k != key);
            self.insertion_order.push_back(key.to_string());
            Some(entry.element.clone())
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        self.elements.len()
    }

    fn max_size(&self) -> usize {
        MAX_CACHE_SIZE
    }

    fn clear(&mut self) {
        self.elements.clear();
        self.insertion_order.clear();
    }

    fn remove(&mut self, key: &str) {
        self.elements.remove(key);
        self.insertion_order.retain(|k| k != key);
    }
}

// Use OnceLock for lazy initialization
static ELEMENT_CACHE: OnceLock<RwLock<ElementCache>> = OnceLock::new();

fn get_cache() -> &'static RwLock<ElementCache> {
    ELEMENT_CACHE.get_or_init(|| RwLock::new(ElementCache::new()))
}

/// Helper: recover from lock poisoning instead of panicking.
fn recover_lock<T>(lock_result: Result<T, std::sync::PoisonError<T>>) -> T {
    lock_result.unwrap_or_else(|e| e.into_inner())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Cache an element by its RuntimeId string.
pub fn cache_element(runtime_id: String, element: UIElement) {
    let mut cache = recover_lock(get_cache().write());
    cache.insert(runtime_id, element);
}

/// Look up a cached element by its RuntimeId string.
/// Returns None if not found, expired, or cache is empty.
/// On hit, promotes the key to most-recently-used position (true LRU).
/// Uses global default TTL for expiry check.
pub fn get_cached_element(runtime_id: &str) -> Option<UIElement> {
    let mut cache = recover_lock(get_cache().write());
    cache.get(runtime_id)
}

/// Look up a cached element with a custom TTL (overrides global default).
/// `ttl` = None means never expire regardless of global setting.
pub fn get_cached_element_with_ttl(
    runtime_id: &str,
    ttl: Option<Duration>,
) -> Option<UIElement> {
    let mut cache = recover_lock(get_cache().write());
    cache.get_with_ttl(runtime_id, ttl)
}

/// Get the number of cached elements.
pub fn cache_size() -> usize {
    let cache = recover_lock(get_cache().read());
    cache.len()
}

/// Get cache statistics: (size, max_size, default_ttl).
pub fn cache_stats() -> (usize, usize, Option<Duration>) {
    let cache = recover_lock(get_cache().read());
    (cache.len(), cache.max_size(), cache.default_ttl)
}

/// Set the global default TTL for cached elements.
/// None = never expire.
pub fn set_default_ttl(ttl: Option<Duration>) {
    let mut cache = recover_lock(get_cache().write());
    cache.default_ttl = ttl;
}

/// Get the global default TTL.
pub fn get_default_ttl() -> Option<Duration> {
    let cache = recover_lock(get_cache().read());
    cache.default_ttl
}

/// Clear the entire element cache.
pub fn clear_cache() {
    let mut cache = recover_lock(get_cache().write());
    cache.clear();
}

/// Remove a specific cached element by runtimeId.
pub fn remove_cached_element(runtime_id: &str) {
    let mut cache = recover_lock(get_cache().write());
    cache.remove(runtime_id);
}
