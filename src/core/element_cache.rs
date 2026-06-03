// src/core/element_cache.rs
//
// Thread-safe element cache for find-from-element API.
// Uses uiautomation-rs UIElement type (Send + Sync under MTA).
//
// Optimizations:
// - VecDeque instead of Vec for O(1) popleft eviction
// - RwLock instead of Mutex for read-heavy workload
// - Graceful lock poisoning recovery

use std::collections::{HashMap, VecDeque};
use std::sync::{OnceLock, RwLock};
use uiautomation::core::UIElement;

/// Maximum number of cached elements before eviction.
const MAX_CACHE_SIZE: usize = 512;

struct ElementCache {
    /// RuntimeId string → cached UIElement reference.
    elements: HashMap<String, UIElement>,
    /// Insertion order for LRU eviction (VecDeque for O(1) pop_front).
    insertion_order: VecDeque<String>,
}

impl ElementCache {
    fn new() -> Self {
        Self {
            elements: HashMap::new(),
            insertion_order: VecDeque::new(),
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

        self.elements.insert(key.clone(), element);
        self.insertion_order.push_back(key);
    }

    fn get(&self, key: &str) -> Option<UIElement> {
        self.elements.get(key).cloned()
    }

    fn len(&self) -> usize {
        self.elements.len()
    }

    fn clear(&mut self) {
        self.elements.clear();
        self.insertion_order.clear();
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
/// Returns None if not found or cache is empty.
pub fn get_cached_element(runtime_id: &str) -> Option<UIElement> {
    let cache = recover_lock(get_cache().read());
    cache.get(runtime_id)
}

/// Get the number of cached elements.
pub fn cache_size() -> usize {
    let cache = recover_lock(get_cache().read());
    cache.len()
}

/// Clear the entire element cache.
pub fn clear_cache() {
    let mut cache = recover_lock(get_cache().write());
    cache.clear();
}
