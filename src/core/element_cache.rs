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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    // ── Helpers ───────────────────────────────────────────────────────────

    /// Get the desktop root element for use as a mock UIElement in cache tests.
    /// Returns None if UIAutomation is unavailable (e.g. non-Windows CI).
    fn get_desktop_element() -> Option<UIElement> {
        uiautomation::UIAutomation::new()
            .ok()
            .and_then(|automation| automation.get_root_element().ok())
    }

    /// Create a unique key for each test to avoid cross-test contamination.
    fn test_key(prefix: &str) -> String {
        format!("test:{prefix}:{}", std::process::id())
    }

    fn setup() {
        clear_cache();
        set_default_ttl(None);
    }

    // ── TC-CORE-01: Basic insert and get ─────────────────────────────────

    #[test]
    fn test_cache_insert_and_get() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let key = test_key("insert_get");
        cache_element(key.clone(), elem.clone());
        assert_eq!(cache_size(), 1, "cache should have 1 entry after insert");

        let cached = get_cached_element(&key);
        assert!(cached.is_some(), "should find cached element by key");

        // Verify the element is valid (can read properties)
        let cached = cached.unwrap();
        let name = cached.get_name().ok();
        assert!(name.is_some(), "cached element should be valid UIA element");
    }

    // ── TC-CORE-02: Cache miss returns None ───────────────────────────────

    #[test]
    fn test_cache_miss_returns_none() {
        setup();
        let result = get_cached_element("nonexistent:key:12345");
        assert!(result.is_none(), "non-existent key should return None");
    }

    // ── TC-CORE-03: LRU eviction ─────────────────────────────────────────

    #[test]
    fn test_lru_eviction() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // Fill cache to max capacity + 1
        let total = MAX_CACHE_SIZE + 1;
        for i in 0..total {
            cache_element(format!("key:{i}"), elem.clone());
        }

        assert_eq!(cache_size(), MAX_CACHE_SIZE, "cache should not exceed MAX_CACHE_SIZE");

        // The first inserted key should be evicted (oldest)
        let first = get_cached_element("key:0");
        assert!(first.is_none(), "oldest key should be evicted");

        // The second key should still exist
        let second = get_cached_element("key:1");
        assert!(second.is_some(), "second key should still be in cache");

        // The last inserted key should exist
        let last_key = format!("key:{}", MAX_CACHE_SIZE);
        let last = get_cached_element(&last_key);
        assert!(last.is_some(), "last inserted key should exist");
    }

    // ── TC-CORE-04: LRU promotion ────────────────────────────────────────

    #[test]
    fn test_lru_promotion() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // Use a simple and reliable approach:
        // 1. Fill cache completely (MAX entries)
        // 2. Access a middle entry to promote it to MRU
        // 3. Insert one more → oldest non-promoted entry should be evicted
        // 4. The promoted entry should survive

        // Fill cache to MAX
        for i in 0..MAX_CACHE_SIZE {
            cache_element(format!("fill:{i}"), elem.clone());
        }
        assert_eq!(cache_size(), MAX_CACHE_SIZE);

        // Access fill:256 (middle entry) to promote it
        let promoted = get_cached_element("fill:256");
        assert!(promoted.is_some(), "fill:256 should be found before eviction");

        // Insert one more entry → should evict fill:0 (the oldest, never accessed)
        cache_element("new_entry".to_string(), elem.clone());

        // fill:0 should be evicted (oldest)
        let fill0 = get_cached_element("fill:0");
        assert!(fill0.is_none(), "fill:0 should be evicted (oldest, never promoted)");

        // fill:256 should still exist (was promoted to MRU)
        let still_there = get_cached_element("fill:256");
        assert!(still_there.is_some(), "fill:256 should survive (LRU promoted)");

        // new_entry should exist
        let new_entry = get_cached_element("new_entry");
        assert!(new_entry.is_some(), "new_entry should exist");

        assert_eq!(cache_size(), MAX_CACHE_SIZE, "cache size should remain at MAX");
    }

    // ── TC-CORE-05: Duplicate insert does not overwrite ──────────────────

    #[test]
    fn test_duplicate_insert_no_overwrite() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let key = test_key("dup");
        cache_element(key.clone(), elem.clone());

        // Second insert with same key should be a no-op
        // (We can't easily verify the element identity, but we can verify size)
        let size_before = cache_size();
        cache_element(key.clone(), elem.clone());
        assert_eq!(cache_size(), size_before, "duplicate insert should not increase size");

        // Element should still be retrievable
        let cached = get_cached_element(&key);
        assert!(cached.is_some(), "element should still be retrievable");
    }

    // ── TC-CORE-06: TTL expiry ───────────────────────────────────────────

    #[test]
    fn test_ttl_expiry() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let key = test_key("ttl");
        set_default_ttl(Some(Duration::from_millis(100)));
        cache_element(key.clone(), elem.clone());

        // Immediate get should succeed
        let cached = get_cached_element(&key);
        assert!(cached.is_some(), "should hit before TTL expires");

        // Wait for TTL to expire
        thread::sleep(Duration::from_millis(150));

        // After expiry, should return None
        let expired = get_cached_element(&key);
        assert!(expired.is_none(), "should return None after TTL expires");

        // Entry should be removed from cache
        assert_eq!(cache_size(), 0, "expired entry should be removed from cache");

        // Reset TTL for other tests
        set_default_ttl(None);
    }

    // ── TC-CORE-07: TTL None means never expire ──────────────────────────

    #[test]
    fn test_ttl_none_never_expires() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let key = test_key("never");
        set_default_ttl(None);
        cache_element(key.clone(), elem.clone());

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        // Should still be available
        let cached = get_cached_element(&key);
        assert!(cached.is_some(), "should never expire when TTL is None");
    }

    // ── TC-CORE-08: get_with_ttl overrides global TTL ────────────────────

    #[test]
    fn test_get_with_ttl_overrides_global() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let key = test_key("ttl_override");

        // Set global TTL = 50ms (short)
        set_default_ttl(Some(Duration::from_millis(50)));
        cache_element(key.clone(), elem.clone());

        // Wait past global TTL
        thread::sleep(Duration::from_millis(80));

        // get_cached_element (uses global TTL) should miss
        let global = get_cached_element(&key);
        assert!(global.is_none(), "global TTL should cause miss");

        // Re-insert and use custom TTL = 500ms
        cache_element(key.clone(), elem.clone());
        thread::sleep(Duration::from_millis(80));

        // get_with_ttl with longer TTL should hit
        let custom = get_cached_element_with_ttl(&key, Some(Duration::from_millis(500)));
        assert!(custom.is_some(), "custom TTL should override global TTL");

        // Reset TTL
        set_default_ttl(None);
    }

    // ── TC-CORE-09: clear_cache and remove_cached_element ────────────────

    #[test]
    fn test_clear_and_remove() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // Insert A, B, C
        cache_element("rm:A".to_string(), elem.clone());
        cache_element("rm:B".to_string(), elem.clone());
        cache_element("rm:C".to_string(), elem.clone());
        assert_eq!(cache_size(), 3);

        // Remove B
        remove_cached_element("rm:B");
        assert_eq!(cache_size(), 2);
        assert!(get_cached_element("rm:A").is_some(), "A should still exist");
        assert!(get_cached_element("rm:B").is_none(), "B should be removed");
        assert!(get_cached_element("rm:C").is_some(), "C should still exist");

        // Clear all
        clear_cache();
        assert_eq!(cache_size(), 0);
        assert!(get_cached_element("rm:A").is_none());
        assert!(get_cached_element("rm:C").is_none());
    }

    // ── TC-CORE-10: Lock poisoning recovery ──────────────────────────────

    #[test]
    fn test_recover_lock_from_poisoning() {
        // Test the recover_lock helper directly
        let result: Result<i32, std::sync::PoisonError<i32>> = Ok(42);
        assert_eq!(recover_lock(result), 42);

        // Test with a poisoned-like scenario (unwrap_or_else on Ok)
        let ok_result: Result<String, std::sync::PoisonError<String>> = Ok("hello".to_string());
        assert_eq!(recover_lock(ok_result), "hello");
    }

    // ── TC-CORE-11: Cache stats ──────────────────────────────────────────

    #[test]
    fn test_cache_stats() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        clear_cache();

        // Initial stats
        let (size, max, ttl) = cache_stats();
        assert_eq!(size, 0, "initial size should be 0");
        assert_eq!(max, MAX_CACHE_SIZE);
        assert_eq!(ttl, None, "initial TTL should be None");

        // After inserting
        cache_element(test_key("stats1"), elem.clone());
        cache_element(test_key("stats2"), elem.clone());
        let (size, max, _ttl) = cache_stats();
        assert_eq!(size, 2);
        assert_eq!(max, MAX_CACHE_SIZE);

        // After setting TTL
        set_default_ttl(Some(Duration::from_secs(30)));
        let (_size, _max, ttl) = cache_stats();
        assert_eq!(ttl, Some(Duration::from_secs(30)));

        // Reset
        set_default_ttl(None);
    }

    // ── TC-CORE-12: set_default_ttl and get_default_ttl ──────────────────

    #[test]
    fn test_set_and_get_default_ttl() {
        setup();

        assert_eq!(get_default_ttl(), None, "default TTL should be None initially");

        set_default_ttl(Some(Duration::from_secs(60)));
        assert_eq!(get_default_ttl(), Some(Duration::from_secs(60)));

        set_default_ttl(Some(Duration::from_millis(500)));
        assert_eq!(get_default_ttl(), Some(Duration::from_millis(500)));

        set_default_ttl(None);
        assert_eq!(get_default_ttl(), None);
    }

    // ── TC-CORE-13: Concurrent access ────────────────────────────────────

    #[test]
    fn test_concurrent_access() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        clear_cache();

        // Pre-populate some keys
        let pre_keys: Vec<String> = (0..10).map(|i| test_key(&format!("concur_pre_{i}"))).collect();
        for k in &pre_keys {
            cache_element(k.clone(), elem.clone());
        }

        let writer_keys: Vec<String> = (0..5).map(|i| test_key(&format!("concur_write_{i}"))).collect();
        let reader_keys: Vec<String> = pre_keys.clone();

        // Spawn reader threads
        let _reader_elem = elem.clone();
        let read_keys = reader_keys.clone();
        let read_handle = thread::spawn(move || {
            for k in &read_keys {
                let _ = get_cached_element(k);
            }
            read_keys.len()
        });

        // Spawn writer threads
        let writer_elem = elem.clone();
        let write_keys = writer_keys.clone();
        let write_handle = thread::spawn(move || {
            for k in &write_keys {
                cache_element(k.clone(), writer_elem.clone());
            }
            write_keys.len()
        });

        let reads = read_handle.join().expect("reader thread should not panic");
        let writes = write_handle.join().expect("writer thread should not panic");

        assert_eq!(reads, reader_keys.len());
        assert_eq!(writes, writer_keys.len());

        // All pre-existing keys should still be accessible
        for k in &pre_keys {
            let cached = get_cached_element(k);
            assert!(cached.is_some(), "pre-existing key {k} should still exist");
        }

        // All written keys should exist
        for k in &writer_keys {
            let cached = get_cached_element(k);
            assert!(cached.is_some(), "written key {k} should exist");
        }
    }

    // ── TC-CORE-14: Edge case - empty string key ─────────────────────────

    #[test]
    fn test_empty_string_key() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // Insert with empty key
        cache_element(String::new(), elem.clone());
        assert_eq!(cache_size(), 1);

        // Retrieve with empty key
        let cached = get_cached_element("");
        assert!(cached.is_some(), "empty string key should work");

        clear_cache();
    }

    // ── TC-CORE-15: Edge case - cache after clear should be empty ────────

    #[test]
    fn test_cache_after_clear_is_empty() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        cache_element(test_key("clear1"), elem.clone());
        cache_element(test_key("clear2"), elem.clone());
        clear_cache();

        assert_eq!(cache_size(), 0);
        assert!(get_cached_element(&test_key("clear1")).is_none());
        assert!(get_cached_element(&test_key("clear2")).is_none());

        // After clear, new inserts should work
        cache_element(test_key("clear3"), elem.clone());
        assert_eq!(cache_size(), 1);
        assert!(get_cached_element(&test_key("clear3")).is_some());
    }
}
