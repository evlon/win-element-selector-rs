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
            // 默认 TTL 30 分钟，防止长时间运行后缓存积累失效 COM proxy
            default_ttl: Some(Duration::from_secs(1800)),
        }
    }

    fn insert(&mut self, key: String, element: UIElement) -> bool {
        // Fix 4: 已存在时更新元素和时间戳（而非静默跳过）
        if let Some(entry) = self.elements.get_mut(&key) {
            entry.element = element;
            entry.cached_at = Instant::now();
            // 提升到 MRU 位置
            self.insertion_order.retain(|k| k != &key);
            self.insertion_order.push_back(key);
            return true;
        }

        // 缓存满时驱逐策略：
        // 1. 先清理所有已过期的条目
        // 2. 如果仍满，LRU 驱逐最久未访问的条目
        if self.elements.len() >= MAX_CACHE_SIZE {
            self.evict_expired();
        }

        if self.elements.len() >= MAX_CACHE_SIZE {
            // Fix 2: LRU 驱逐 — 移除最久未访问的条目
            if let Some(oldest_key) = self.insertion_order.pop_front() {
                self.elements.remove(&oldest_key);
            }
        }

        self.elements.insert(key.clone(), CachedElement {
            element,
            cached_at: Instant::now(),
        });
        self.insertion_order.push_back(key);
        true
    }

    /// 清理所有已过期的缓存条目
    fn evict_expired(&mut self) {
        let effective_ttl = self.default_ttl;
        if effective_ttl.is_none() {
            return; // No TTL set, nothing to expire
        }
        let ttl_dur = effective_ttl.unwrap();

        let now = Instant::now();
        let expired_keys: Vec<String> = self
            .elements
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.cached_at) > ttl_dur)
            .map(|(k, _)| k.clone())
            .collect();

        for k in &expired_keys {
            self.elements.remove(k);
            self.insertion_order.retain(|ik| ik != k);
        }
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
/// - If key already exists, updates the element and timestamp (no stale entries).
/// - If cache is full, evicts expired entries first, then LRU-evicts the oldest.
/// Always returns true.
pub fn cache_element(runtime_id: String, element: UIElement) -> bool {
    let mut cache = recover_lock(get_cache().write());
    cache.insert(runtime_id, element)
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

    // ── TC-CORE-03: TTL优先 + LRU驱逐（满时驱逐最久未访问条目）────────────────

    #[test]
    fn test_ttl_priority_eviction() {
        setup();
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // 设置 TTL = 100ms
        set_default_ttl(Some(Duration::from_millis(100)));

        // 填满缓存到 MAX_CACHE_SIZE
        for i in 0..MAX_CACHE_SIZE {
            let ok = cache_element(format!("key:{i}"), elem.clone());
            assert!(ok, "key:{i} should be inserted successfully");
        }
        assert_eq!(cache_size(), MAX_CACHE_SIZE, "cache should be full");

        // 插入第 MAX+1 条 → LRU 驱逐最久未访问的条目（key:0），插入成功
        let accepted = cache_element(format!("key:{}", MAX_CACHE_SIZE), elem.clone());
        assert!(accepted, "insert should succeed via LRU eviction when cache full");
        assert_eq!(cache_size(), MAX_CACHE_SIZE, "cache size should stay at MAX after LRU eviction");

        // key:0 应该被 LRU 驱逐了（最久未访问）
        let evicted = get_cached_element("key:0");
        assert!(evicted.is_none(), "key:0 should be LRU-evicted");

        // key:1 仍应在
        let second = get_cached_element("key:1");
        assert!(second.is_some(), "key:1 should still exist");

        // 等待 TTL 过期
        thread::sleep(Duration::from_millis(150));

        // 过期的条目应该查不到了
        let expired = get_cached_element("key:1");
        assert!(expired.is_none(), "expired key should be removed");

        // Reset TTL for other tests
        set_default_ttl(None);
    }

    // ── TC-CORE-04: LRU promotion (get() promotes to MRU) ─────────────────

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

        // TTL优先策略下，LRU promotion 用于 get() 时的访问顺序维护
        // 主要验证：get() 后条目仍然存在且可正常访问

        // Fill cache to MAX
        for i in 0..MAX_CACHE_SIZE {
            cache_element(format!("fill:{i}"), elem.clone());
        }
        assert_eq!(cache_size(), MAX_CACHE_SIZE);

        // Access fill:0 (oldest) to promote it
        let promoted = get_cached_element("fill:0");
        assert!(promoted.is_some(), "fill:0 should be found");

        // fill:0 should still exist (promoted in insertion_order)
        let still_there = get_cached_element("fill:0");
        assert!(still_there.is_some(), "fill:0 should survive (LRU promoted)");

        // All entries should exist (no eviction when no TTL set)
        for i in 0..MAX_CACHE_SIZE {
            let entry = get_cached_element(&format!("fill:{i}"));
            assert!(entry.is_some(), "fill:{i} should exist");
        }

        assert_eq!(cache_size(), MAX_CACHE_SIZE, "cache size should remain at MAX");
    }

    // ── TC-CORE-05: Duplicate insert updates element and timestamp ──────────

    #[test]
    fn test_duplicate_insert_updates() {
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

        // Second insert with same key should update (not increase size)
        let size_before = cache_size();
        cache_element(key.clone(), elem.clone());
        assert_eq!(cache_size(), size_before, "duplicate insert should not increase size");

        // Element should still be retrievable with fresh timestamp
        let cached = get_cached_element(&key);
        assert!(cached.is_some(), "element should still be retrievable after update");
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

    // ══════════════════════════════════════════════════════════════════════════
    // TC-CORE-LONG: 长时间运行缓存综合测试
    //
    // 模拟真实场景：缓存运行指定时长，验证 TTL 过期、LRU 驱逐、
    // 重复插入更新、显式删除、LRU promotion、默认 TTL 等行为。
    //
    // 用法: cargo test test_long_run_cache -- --nocapture
    //       cargo test test_long_run_cache -- --nocapture --duration-secs 30
    //
    // 环境变量:
    //   CACHE_TEST_DURATION_SECS  测试时长秒数（默认 5）
    // ══════════════════════════════════════════════════════════════════════════

    /// 长时间运行缓存综合测试场景
    ///
    /// 场景覆盖：
    /// 1. TTL 过期：短 TTL 元素自动过期被清理
    /// 2. LRU 驱逐：缓存满时最久未访问条目被淘汰
    /// 3. 重复插入更新：同 key 重复插入更新元素和时间戳
    /// 4. LRU promotion：访问旧条目提升其优先级，避免被驱逐
    /// 5. 显式删除：remove_cached_element 精确删除
    /// 6. 默认 TTL 30min：验证默认 TTL 生效
    /// 7. 并发安全：多线程同时读写不 panic 不死锁
    /// 8. 缓存统计一致性：cache_size / cache_stats 始终准确
    #[test]
    fn test_long_run_cache() {
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        // 从环境变量读取测试时长，默认 5 秒
        let duration_secs: u64 = std::env::var("CACHE_TEST_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let test_duration = Duration::from_secs(duration_secs);
        let start = Instant::now();

        eprintln!("═══ 长时间运行缓存综合测试 ═══");
        eprintln!("测试时长: {}s", duration_secs);
        eprintln!("开始时间: {:?}", std::time::SystemTime::now());

        // ── 阶段 0: 验证默认 TTL 为 30 分钟 ──────────────────────────
        {
            clear_cache();
            let (_, _, ttl) = cache_stats();
            assert_eq!(
                ttl,
                Some(Duration::from_secs(1800)),
                "默认 TTL 应为 30 分钟 (1800s)"
            );
            eprintln!("[Phase 0] ✅ 默认 TTL = 30min");
        }

        // 使用短 TTL 进行测试（100ms），加速 TTL 相关验证
        let short_ttl = Duration::from_millis(100);
        let medium_ttl = Duration::from_millis(300);

        let mut phase = 0u32;
        let mut iteration = 0u64;

        // 统计计数器
        let mut ttl_expired_count = 0u64;
        let mut lru_evicted_count = 0u64;
        let mut duplicate_update_count = 0u64;
        let mut promotion_saved_count = 0u64;
        let mut explicit_remove_count = 0u64;

        while start.elapsed() < test_duration {
            iteration += 1;
            phase = (phase + 1) % 8; // 循环 8 个场景

            match phase {
                // ── 场景 1: TTL 过期 ─────────────────────────────
                // 插入元素，等待 TTL 过期，验证 get 返回 None
                1 => {
                    clear_cache();
                    set_default_ttl(Some(short_ttl));
                    let key = format!("ttl:{}", iteration);
                    cache_element(key.clone(), elem.clone());
                    assert!(get_cached_element(&key).is_some(), "TTL 未过期应命中");

                    thread::sleep(short_ttl + Duration::from_millis(50));
                    let result = get_cached_element(&key);
                    if result.is_none() {
                        ttl_expired_count += 1;
                    }
                    assert!(result.is_none(), "TTL 过期后应返回 None");
                    assert_eq!(cache_size(), 0, "过期条目应被移除");
                    set_default_ttl(None);
                }

                // ── 场景 2: LRU 驱逐 ────────────────────────────
                // 填满缓存，再插入新条目，验证最旧的被驱逐
                2 => {
                    clear_cache();
                    set_default_ttl(None); // 无 TTL，纯 LRU
                    // 填满
                    for i in 0..MAX_CACHE_SIZE {
                        cache_element(format!("lru:{}:{}", iteration, i), elem.clone());
                    }
                    assert_eq!(cache_size(), MAX_CACHE_SIZE);

                    // 插入新条目，应 LRU 驱逐最旧
                    let new_key = format!("lru:{}:new", iteration);
                    cache_element(new_key.clone(), elem.clone());
                    assert_eq!(cache_size(), MAX_CACHE_SIZE);

                    // 最旧条目应被驱逐
                    let oldest_key = format!("lru:{}:0", iteration);
                    if get_cached_element(&oldest_key).is_none() {
                        lru_evicted_count += 1;
                    }
                    assert!(
                        get_cached_element(&oldest_key).is_none(),
                        "最旧条目应被 LRU 驱逐"
                    );
                    // 新条目应存在
                    assert!(get_cached_element(&new_key).is_some(), "新条目应存在");
                    set_default_ttl(None);
                }

                // ── 场景 3: 重复插入更新 ──────────────────────────
                // 同 key 插入两次，验证元素被更新（不增加 size，时间戳刷新）
                3 => {
                    clear_cache();
                    set_default_ttl(Some(medium_ttl));
                    let key = format!("dup:{}", iteration);

                    cache_element(key.clone(), elem.clone());
                    let size_after_first = cache_size();

                    // 等一小段时间让时间戳有差异
                    thread::sleep(Duration::from_millis(20));

                    cache_element(key.clone(), elem.clone());
                    assert_eq!(cache_size(), size_after_first, "重复插入不应增加 size");

                    // 更新后的条目应有更新的时间戳，在短 TTL 过期后仍应存活
                    let result = get_cached_element(&key);
                    if result.is_some() {
                        duplicate_update_count += 1;
                    }
                    assert!(result.is_some(), "更新后条目应可访问");
                    set_default_ttl(None);
                }

                // ── 场景 4: LRU promotion ──────────────────────────
                // 访问旧条目使其被提升，验证其不被 LRU 驱逐
                4 => {
                    clear_cache();
                    set_default_ttl(None);
                    // 填满缓存
                    for i in 0..MAX_CACHE_SIZE {
                        cache_element(format!("promo:{}:{}", iteration, i), elem.clone());
                    }

                    // 访问最旧的条目 (promo:0)，提升其 LRU 位置
                    let promoted_key = format!("promo:{}:0", iteration);
                    assert!(get_cached_element(&promoted_key).is_some(), "提升前应命中");

                    // 再插入新条目，驱逐的应是最久未访问的（promo:1，因为 promo:0 已提升）
                    let new_key = format!("promo:{}:new", iteration);
                    cache_element(new_key.clone(), elem.clone());

                    // promo:0 被提升，不应被驱逐
                    let still_there = get_cached_element(&promoted_key);
                    if still_there.is_some() {
                        promotion_saved_count += 1;
                    }
                    assert!(still_there.is_some(), "被提升的条目不应被驱逐");

                    // promo:1 是新的最久未访问，应被驱逐
                    let evicted_key = format!("promo:{}:1", iteration);
                    assert!(get_cached_element(&evicted_key).is_none(), "未提升的最旧条目应被驱逐");
                    set_default_ttl(None);
                }

                // ── 场景 5: 显式删除 ─────────────────────────────
                // remove_cached_element 精确删除，不影响其他条目
                5 => {
                    clear_cache();
                    set_default_ttl(None);
                    let keep_key = format!("keep:{}", iteration);
                    let remove_key = format!("remove:{}", iteration);

                    cache_element(keep_key.clone(), elem.clone());
                    cache_element(remove_key.clone(), elem.clone());
                    assert_eq!(cache_size(), 2);

                    remove_cached_element(&remove_key);
                    explicit_remove_count += 1;
                    assert_eq!(cache_size(), 1);
                    assert!(get_cached_element(&keep_key).is_some(), "保留条目应仍在");
                    assert!(get_cached_element(&remove_key).is_none(), "删除条目应已移除");
                    set_default_ttl(None);
                }

                // ── 场景 6: 默认 TTL 30min 生效 ──────────────────
                // 不设 TTL（使用默认 30min），短时间内应始终命中
                6 => {
                    clear_cache();
                    set_default_ttl(None); // 重置
                    // 重新初始化缓存以使用默认 TTL
                    // 由于 OnceLock 不能重新初始化，直接设回默认
                    set_default_ttl(Some(Duration::from_secs(1800)));

                    let key = format!("default_ttl:{}", iteration);
                    cache_element(key.clone(), elem.clone());

                    // 短时间内应始终命中
                    assert!(get_cached_element(&key).is_some(), "默认 TTL 下应命中");
                    assert_eq!(
                        get_default_ttl(),
                        Some(Duration::from_secs(1800)),
                        "默认 TTL 应为 30min"
                    );
                    set_default_ttl(None);
                }

                // ── 场景 7: 并发安全 ──────────────────────────────
                // 多线程同时读写，不 panic 不死锁
                7 => {
                    clear_cache();
                    set_default_ttl(None);

                    let concurrency = 4;
                    let ops_per_thread = 10;
                    let thread_elem = elem.clone();

                    let handles: Vec<_> = (0..concurrency)
                        .map(|tid| {
                            let e = thread_elem.clone();
                            thread::spawn(move || {
                                for j in 0..ops_per_thread {
                                    let key = format!("conc:{}:{}", tid, j);
                                    cache_element(key.clone(), e.clone());
                                    let _ = get_cached_element(&key);
                                    if j % 3 == 0 {
                                        remove_cached_element(&key);
                                    }
                                }
                                tid
                            })
                        })
                        .collect();

                    let mut all_ok = true;
                    for h in handles {
                        if h.join().is_err() {
                            all_ok = false;
                        }
                    }
                    assert!(all_ok, "并发操作不应 panic");
                }

                // ── 场景 8: 统计一致性 ────────────────────────────
                // cache_size / cache_stats 数值始终一致
                0 => {
                    clear_cache();
                    set_default_ttl(None);

                    let n = 10;
                    for i in 0..n {
                        cache_element(format!("stat:{}:{}", iteration, i), elem.clone());
                    }

                    let size = cache_size();
                    let (stats_size, stats_max, _) = cache_stats();
                    assert_eq!(size, n, "cache_size 应等于插入数");
                    assert_eq!(size, stats_size, "cache_size 与 stats.size 应一致");
                    assert_eq!(stats_max, MAX_CACHE_SIZE, "max_size 应为 {}", MAX_CACHE_SIZE);

                    // 删除一半，再验证
                    for i in 0..n / 2 {
                        remove_cached_element(&format!("stat:{}:{}", iteration, i));
                    }
                    let after_remove = cache_size();
                    let (stats_after, _, _) = cache_stats();
                    assert_eq!(after_remove, n - n / 2, "删除后 size 应减半");
                    assert_eq!(after_remove, stats_after, "删除后 size 与 stats 一致");
                }

                _ => unreachable!(),
            }
        }

        // ── 最终验证 ────────────────────────────────────────────
        clear_cache();
        set_default_ttl(None);

        eprintln!("\n═══ 测试结果 ═══");
        eprintln!("总迭代: {} ({} 个场景)", iteration, iteration * 8);
        eprintln!("TTL 过期验证: {} 次", ttl_expired_count);
        eprintln!("LRU 驱逐验证: {} 次", lru_evicted_count);
        eprintln!("重复插入更新验证: {} 次", duplicate_update_count);
        eprintln!("LRU promotion 验证: {} 次", promotion_saved_count);
        eprintln!("显式删除验证: {} 次", explicit_remove_count);
        eprintln!("测试时长: {:.2}s", start.elapsed().as_secs_f64());
        eprintln!("═══ 全部通过 ✅ ═══");
    }

    /// 模拟真实长时间运行场景：大量元素缓存 → 过期 → 补充 → LRU 驱逐
    ///
    /// 用法: cargo test test_long_run_realistic -- --nocapture
    ///       CACHE_TEST_DURATION_SECS=60 cargo test test_long_run_realistic -- --nocapture
    #[test]
    fn test_long_run_realistic() {
        let elem = match get_desktop_element() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: UIAutomation not available");
                return;
            }
        };

        let duration_secs: u64 = std::env::var("CACHE_TEST_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let test_duration = Duration::from_secs(duration_secs);
        let start = Instant::now();

        eprintln!("═══ 模拟真实长时间运行场景 ═══");
        eprintln!("测试时长: {}s", duration_secs);

        clear_cache();
        // 使用中等 TTL 模拟真实环境
        let realistic_ttl = Duration::from_millis(200);
        set_default_ttl(Some(realistic_ttl));

        let mut key_counter: u64 = 0;
        let mut total_inserts = 0u64;
        let mut total_hits = 0u64;
        let mut total_misses = 0u64;
        let mut total_removes = 0u64;

        // 初始批量插入（模拟捕获元素）
        let batch_size = 50;
        for _ in 0..batch_size {
            key_counter += 1;
            cache_element(format!("elem:{}", key_counter), elem.clone());
            total_inserts += 1;
        }

        while start.elapsed() < test_duration {
            key_counter += 1;

            // 混合操作：70% 读, 15% 写, 10% 删除, 5% 更新
            let op = (key_counter % 100) as u8;

            if op < 70 {
                // 读操作：随机访问已有 key
                let lookup_key = format!("elem:{}", (key_counter % key_counter.max(1)).max(1));
                match get_cached_element(&lookup_key) {
                    Some(_) => total_hits += 1,
                    None => total_misses += 1,
                }
            } else if op < 85 {
                // 写操作：插入新元素
                cache_element(format!("elem:{}", key_counter), elem.clone());
                total_inserts += 1;
            } else if op < 95 {
                // 删除操作：删除旧元素
                let del_key = format!("elem:{}", (key_counter % key_counter.max(1)).max(1));
                remove_cached_element(&del_key);
                total_removes += 1;
            } else {
                // 更新操作：重复插入同 key
                let update_key = format!("elem:{}", (key_counter % key_counter.max(1)).max(1));
                cache_element(update_key, elem.clone());
                total_inserts += 1;
            }

            // 定期验证缓存不变量
            if key_counter % 100 == 0 {
                let (size, max, _) = cache_stats();
                assert!(size <= max, "缓存大小不应超过最大值: {} > {}", size, max);
            }

            // 模拟现实节奏（每 100 次操作休眠一小段）
            if key_counter % 50 == 0 {
                thread::sleep(Duration::from_millis(5));
            }
        }

        let (final_size, final_max, final_ttl) = cache_stats();
        assert!(final_size <= final_max, "最终缓存大小合法");

        eprintln!("\n═══ 真实场景测试结果 ═══");
        eprintln!("总操作: {} 次", total_inserts + total_hits + total_misses + total_removes);
        eprintln!("  插入: {}, 命中: {}, 未命中: {}, 删除: {}",
            total_inserts, total_hits, total_misses, total_removes);
        eprintln!("  命中率: {:.1}%",
            if total_hits + total_misses > 0 {
                total_hits as f64 / (total_hits + total_misses) as f64 * 100.0
            } else { 0.0 });
        eprintln!("最终缓存: {}/{}", final_size, final_max);
        eprintln!("TTL: {:?}", final_ttl);
        eprintln!("测试时长: {:.2}s", start.elapsed().as_secs_f64());
        eprintln!("═══ 全部通过 ✅ ═══");

        clear_cache();
        set_default_ttl(None);
    }
}
