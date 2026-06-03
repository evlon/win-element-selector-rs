use super::*;

pub(super) const XPATH_FALLBACK_BUDGET_MS: u128 = 3000;

/// The strategy that successfully resolved an XPath against a window.
#[derive(Debug, Clone)]
pub(super) enum CompiledStrategy {
    /// Window fast path — XPath targets Window element directly
    WindowFastPath,
    /// Strategy 1: uiauto-xpath from window root (ControlViewWalker)
    ControlViewDirect,
    /// Strategy 1.5: RawViewWalker BFS from window root
    RawViewBfs,
    /// Strategy 2: Search from content root
    ContentRoot,
    /// Strategy 2.5: FindAll(Descendants) raw tree search
    FindAllDescendants,
    /// Strategy 2.7: EnumChildWindows — found on child HWND
    /// Contains the index of the matching child HWND (for direct reuse)
    ChildHwndEnum(usize),
    /// Strategy 3: Sibling window search
    SiblingWindow,
    /// Strategy 3b: Child process window search
    ChildProcessWindow,
    /// //XPath descendant: content root
    DescendantContentRoot,
    /// //XPath descendant: uiauto-xpath from window root
    DescendantWindowRoot,
    /// //XPath descendant: raw descendants
    DescendantRawWalk,
    /// //XPath descendant: child HWND
    DescendantChildHwnd(usize),
}

/// A compiled XPath entry — the winning strategy and performance stats.
#[derive(Debug, Clone)]
pub(super) struct CompiledXPathEntry {
    pub(super) strategy: CompiledStrategy,
    /// Average execution time in milliseconds (exponentially weighted)
    pub(super) avg_time_ms: u64,
    /// Number of cache hits
    pub(super) hit_count: u64,
    /// When this entry was created (for eviction decisions)
    pub(super) created_at: std::time::Instant,
    /// When this entry was last used
    pub(super) last_used: std::time::Instant,
}

static XPATH_CACHE: std::sync::LazyLock<Mutex<HashMap<(u64, String), CompiledXPathEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Maximum number of entries in the cache before eviction starts.
const XPATH_CACHE_MAX_ENTRIES: usize = 256;

/// Build a cache key from an XPath string and a window element.
/// Uses the window's class name prefix (first 32 chars of the part before any underscore)
/// to capture the app type without tying to a specific window instance.
fn cache_key(xpath: &str, window: &UIElement) -> Option<(u64, String)> {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let class = window.get_classname().unwrap_or_default();
    // Extract the "app type" prefix: e.g., "Chrome_WidgetWin_0" → "Chrome_Widget"
    // "mmui::MainWindow" → "mmui"
    let app_prefix = if let Some(pos) = class.find('_') {
        class[..pos].to_string()
    } else if let Some(pos) = class.find("::") {
        class[..pos].to_string()
    } else {
        class
    };

    // Hash the XPath for compact key storage
    let mut hasher = DefaultHasher::new();
    xpath.hash(&mut hasher);
    let xpath_hash = hasher.finish();

    Some((xpath_hash, app_prefix))
}

pub(super) fn cache_lookup(xpath: &str, window: &UIElement) -> Option<CompiledXPathEntry> {
    let key = cache_key(xpath, window)?;
    let mut cache = XPATH_CACHE.lock().ok()?;
    if let Some(entry) = cache.get_mut(&key) {
        entry.hit_count += 1;
        entry.last_used = std::time::Instant::now();
        log::info!(
            "[XPath Cache] HIT: xpath_hash={} app='{}' strategy={:?} avg={}ms hits={}",
            key.0, key.1, entry.strategy, entry.avg_time_ms, entry.hit_count
        );
        Some(entry.clone())
    } else {
        log::info!("[XPath Cache] MISS: xpath_hash={} app='{}'", key.0, key.1);
        None
    }
}

pub(super) fn cache_store(
    xpath: &str,
    window: &UIElement,
    strategy: CompiledStrategy,
    elapsed_ms: u64,
) {
    let key = match cache_key(xpath, window) {
        Some(k) => k,
        None => return,
    };

    let mut cache = match XPATH_CACHE.lock() {
        Ok(c) => c,
        Err(_) => return,
    };

    // Evict oldest entries if cache is full
    if cache.len() >= XPATH_CACHE_MAX_ENTRIES {
        // Find and remove the least recently used entry
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest_key);
            log::info!("[XPath Cache] Evicted LRU entry (cache full at {})", XPATH_CACHE_MAX_ENTRIES);
        }
    }

    let now = std::time::Instant::now();
    let entry = if let Some(existing) = cache.get(&key) {
        // Update existing entry: exponential moving average for time
        let alpha = 0.3f64; // weight for new sample
        let new_avg = (existing.avg_time_ms as f64 * (1.0 - alpha) + elapsed_ms as f64 * alpha) as u64;
        CompiledXPathEntry {
            strategy,
            avg_time_ms: new_avg,
            hit_count: existing.hit_count,
            created_at: existing.created_at,
            last_used: now,
        }
    } else {
        CompiledXPathEntry {
            strategy,
            avg_time_ms: elapsed_ms,
            hit_count: 0,
            created_at: now,
            last_used: now,
        }
    };

    log::info!(
        "[XPath Cache] STORE: xpath_hash={} app='{}' strategy={:?} time={}ms",
        key.0, key.1, entry.strategy, elapsed_ms
    );
    cache.insert(key, entry);
}

pub fn clear_xpath_cache() {
    if let Ok(mut cache) = XPATH_CACHE.lock() {
        let count = cache.len();
        cache.clear();
        log::info!("[XPath Cache] Cleared {} entries", count);
    }
}

pub fn xpath_cache_stats() -> (usize, u64) {
    if let Ok(cache) = XPATH_CACHE.lock() {
        let count = cache.len();
        let total_hits: u64 = cache.values().map(|e| e.hit_count).sum();
        (count, total_hits)
    } else {
        (0, 0)
    }
}

pub(super) struct ParsedXPathStep {
    pub(super) type_name: Option<String>,
    pub(super) required_props: Vec<(String, String)>,
    pub(super) require_starts_with: Vec<(String, String)>,
    pub(super) require_contains: Vec<(String, String)>,
    pub(super) require_matches: Vec<(String, regex::Regex)>,
}

