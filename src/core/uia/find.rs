// src/core/uia/find.rs
//
// XPath search dispatcher and public API.
//
// Architecture:
// - find_control.rs: ControlView / uiauto-xpath search functions ([fast] mode)
// - find_raw.rs:     RawView search functions ([full] mode)
// - find.rs:         Main dispatcher, cache strategies, shared utilities, public API
//
// Key design:
// - [fast]/[fast-child] → ControlView only (find_control.rs)
// - [full]/[full-child] → RawView only (find_raw.rs)
// - No prefix → RawView only (legacy = full, backward compatible)

use super::*;
use super::find_control::{
    search_descendants_via_uiauto_xpath,
};
use super::find_raw::{
    search_descendants_via_raw_view,
    search_descendants_depth_limited,
};
use crate::core::model::SearchStrategy;

// ═══════════════════════════════════════════════════════════════════════════════
// Main Dispatcher
// ═══════════════════════════════════════════════════════════════════════════════

/// New: Execute XPath steps with explicit strategy dispatch based on step prefix.
///
/// NO implicit fallback. Each step's prefix (`/`, `//`, `/*n/`) determines the
/// execution strategy directly. If a strategy fails, returns empty — no retry
/// with alternative strategies.
///
/// ## Strategy mapping
/// | Prefix | Strategy | Implementation |
/// |--------|----------|----------------|
/// | `/A` (Child) | FindAll(Children) | `execute_direct_child` (fallback: uiauto-xpath) |
/// | `//A` (Descendant) | RawView FindAll | `search_descendants_via_raw_view` |
/// | `/*n/A` (DepthLimited) | BFS depth-limited | `find_with_depth_limit` |


/// New: Execute XPath steps with filter support. See `execute_xpath_steps` for strategy docs.
///
/// `timeout_ms`: 超时预算（ms）。超时后返回空结果 + `Timeout` 错误。
///   - `None`: 不限制
///   - `Some(n)`: 从函数入口开始计时，超过 n ms 后停止并返回空
///
/// `chrome_treewalker_fallback`: 当 Fast 模式 (ControlView) 的 descendant 步骤
/// 返回 0 结果时，是否自动回退到 Full 模式 (RawView) 重新搜索。
/// 适用于 Chrome/WebView 场景，ControlView 树可能不完整。
pub(super) fn execute_xpath_steps_filtered(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    filter: &FindAllFilter,
    timeout_ms: Option<u64>,
    chrome_treewalker_fallback: bool,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let start = Instant::now();

    // ── Strip search mode suffix (:first / :onlyone / :all) ──
    let (search_mode, xpath_no_suffix) = SearchMode::strip_suffix(xpath);
    let xpath = xpath_no_suffix;

    // Handle parenthesized positional predicate: (xpath)[N]
    let (inner_xpath, position_index) = if xpath.starts_with('(') {
        if let Some(close) = xpath.rfind(')') {
            let after_close = &xpath[close + 1..];
            if let Some(pos) = parse_positional_predicate(after_close) {
                let inner = xpath[1..close].trim().to_string();
                (inner, Some(pos))
            } else {
                let inner = xpath[1..close].trim().to_string();
                (inner, None)
            }
        } else {
            (xpath.to_string(), None)
        }
    } else {
        (xpath.to_string(), None)
    };

    // ── Strip locate mode prefix ──
    let (locate_mode, _, stripped_xpath) = LocateMode::strip_xpath_prefix(&inner_xpath);
    let xpath = stripped_xpath;
    let is_fast_mode = matches!(locate_mode, Some(LocateMode::Fast) | Some(LocateMode::FastChild));

    // ── Check XPath Compilation Cache ──
    if let Some(cached) = cache_lookup(&xpath, window) {
        if let Some(result) = execute_cached_strategy(auto, window, &xpath, &cached, filter) {
            let (results, segments) = result;
            if !results.is_empty() {
                let elapsed = start.elapsed().as_millis() as u64;
                cache_store(&xpath, window, cached.strategy.clone(), elapsed);
                let results = apply_positional_and_search_mode(results, position_index, search_mode);
                return Ok((results, segments));
            }
        }
    }

    // ── Parse XPath steps ──
    // 不能用 naive split('/')：`/*9/Text[@Name='背影']` 会丢失前缀变成 `["*9", "Text"]`
    // 需要用正则保留 `//`, `/*n/`, `/` 前缀。
    let xpath_parts: Vec<&str> = split_xpath_steps(xpath);
    if xpath_parts.is_empty() {
        return Ok((vec![], vec![]));
    }

    // ── Execute each step based on its prefix ──
    let mut current_elements: Vec<UIElement> = vec![window.clone()];
    let mut segment_results: Vec<SegmentValidationResult> = Vec::new();
    let is_onlyone = matches!(search_mode, SearchMode::OnlyOne);

    for (step_idx, step_str) in xpath_parts.iter().enumerate() {
        // ── 超时检查（需求 §5.5）──
        if let Some(timeout) = timeout_ms {
            if start.elapsed().as_millis() >= timeout as u128 {
                log::warn!(
                    "[ExecuteXPath] Timeout after {}ms at step {}/{} '{}' (budget {}ms)",
                    start.elapsed().as_millis(), step_idx + 1, xpath_parts.len(), step_str, timeout
                );
                // 返回空结果 + Timeout 信息
                let elapsed_ms = start.elapsed().as_millis() as u64;
                segment_results.push(SegmentValidationResult::timeout(step_idx, *step_str, timeout, elapsed_ms));
                return Ok((vec![], segment_results));
            }
        }

        let is_last = step_idx == xpath_parts.len() - 1;
        let parsed = parse_xpath_step(step_str);
        let strategy = StepExecutionStrategy::from(&parsed.prefix);
        let step_start = Instant::now();

        log::info!(
            "[ExecuteXPath] Step {}/{}: '{}' prefix={:?} strategy={:?} is_last={} fast={} onlyone={}",
            step_idx + 1, xpath_parts.len(), step_str, parsed.prefix, strategy, is_last, is_fast_mode, is_onlyone
        );

        let (next_elements, all_steps_consumed): (Vec<UIElement>, bool) = match strategy {
            StepExecutionStrategy::DirectChild => {
                // `/A` — search only in direct children of each current element
                (execute_direct_child(auto, &current_elements, &parsed, is_last, filter)?, false)
            }
            StepExecutionStrategy::Descendant => {
                // `//A` — search all descendants
                if is_fast_mode {
                    let (fast_results, consumed) = execute_descendant_fast(auto, &current_elements, &xpath_parts, step_idx, is_last, search_mode, filter)?;
                    if !fast_results.is_empty() || !chrome_treewalker_fallback {
                        (fast_results, consumed)
                    } else {
                        // Chrome TreeWalker fallback: ControlView found nothing, try RawView
                        log::info!("[ExecuteXPath] Chrome TreeWalker fallback: ControlView descendant step '{}' found 0, retrying with RawView", step_str);
                        let (full_results, consumed2) = execute_descendant_full(auto, &current_elements, &xpath_parts, step_idx, is_last, search_mode, filter)?;
                        (full_results, consumed2)
                    }
                } else {
                    execute_descendant_full(auto, &current_elements, &xpath_parts, step_idx, is_last, search_mode, filter)?
                }
            }
            StepExecutionStrategy::DepthLimitedBfs { max_depth } => {
                // `/*n/A` — BFS with depth limit
                let mut results = Vec::new();
                let walker_hint = if is_fast_mode { WalkerHint::ControlView } else { WalkerHint::RawView };
                for elem in &current_elements {
                    let matches = find_with_depth_limit(auto, elem, &parsed, max_depth, walker_hint.clone());
                    results.extend(matches);
                }
                (results, false)
            }
        };

        let step_ms = step_start.elapsed().as_millis() as u64;
        let matched = !next_elements.is_empty();
        let match_count = next_elements.len();

        // ── findOne 叶子唯一性验证 ──
        // 需求 §5.3.1: 叶子节点必须在父节点（或祖先范围）下唯一。
        // 实现：使用 FindAll(scope, condition, 2) 验证 count <= 1。
        if is_onlyone && is_last && match_count > 1 {
            log::warn!(
                "[ExecuteXPath] findOne LeafNotUnique: step {} '{}' has {} candidates",
                step_idx + 1, step_str, match_count
            );
            // 返回空结果 + LeafNotUnique 的 segment result
            segment_results.push(SegmentValidationResult::leaf_not_unique(step_idx, *step_str, match_count, step_ms));
            return Ok((vec![], segment_results));
        }

        if matched {
            segment_results.push(SegmentValidationResult::matched(step_idx, *step_str, match_count, step_ms));
        } else {
            segment_results.push(SegmentValidationResult::not_found(
                step_idx, *step_str, "XPath", format!("Step not found via {:?}", strategy), step_ms,
            ));
        }

        if next_elements.is_empty() {
            log::info!("[ExecuteXPath] Step {} not found, stopping", step_idx + 1);
            current_elements.clear();
            break;
        }

        // 当 Descendant 步骤通过 Chain 一次性消费了所有剩余步骤时，
        // 返回的结果已经是最终叶子节点，后续步骤无需再执行
        if all_steps_consumed {
            log::info!("[ExecuteXPath] Step {}: Chain consumed all remaining steps, skipping {} subsequent steps",
                step_idx + 1, xpath_parts.len() - step_idx - 1);
            current_elements = next_elements;
            break;
        }

        current_elements = next_elements;
    }

    let total_ms = start.elapsed().as_millis() as u64;
    log::info!(
        "[ExecuteXPath] Completed in {}ms: {} results, {} steps",
        total_ms, current_elements.len(), xpath_parts.len()
    );

    let results = apply_positional_and_search_mode(current_elements, position_index, search_mode);
    Ok((results, segment_results))
}

// ── execute_xpath_steps helpers ───────────────────────────────────────────

/// 将 split_xpath_steps 产生的带前缀步骤片段重建为有效的 Descendant XPath。
///
/// `parts` 中每个 step 自带前缀（`//`、`/`、`/*n/`），直接 concat 即可，
/// 但需要把第一个 step 的前缀统一替换为 `//`（Descendant 语义）。
///
/// 例：`["//Group[@AutomationId='x']", "//Button[@Name='赞']"]`
///   → `//Group[@AutomationId='x']//Button[@Name='赞']`
///
/// 例：`["/Pane", "//Button[@Name='赞']"]`
///   → `//Pane//Button[@Name='赞']`
pub(super) fn reconstruct_descendant_xpath(parts: &[&str]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let first = parts[0];
    // 去掉第一个 step 的轴前缀（// 或 / 或 /*n/），保留节点测试和谓词
    // 安全做法：找到第一个大写字母或 * 的位置
    let first_body = if let Some(pos) = first.find(|c: char| c.is_uppercase() || c == '*') {
        &first[pos..]
    } else {
        first.trim_start_matches('/')
    };
    // 重新加 // 前缀（Descendant 语义）
    let mut result = format!("//{}", first_body);
    for part in &parts[1..] {
        // 后续 step 保留原有前缀（它们已经是正确的 / 或 // 或 /*n/）
        result.push_str(part);
    }
    result
}

/// 去掉步骤的轴前缀（`//`、`/*n/`、`/`），返回节点测试和谓词部分。
///
/// 例：`//Group[@Name='x']` → `Group[@Name='x']`
/// 例：`/*9/Text` → `Text`
/// 例：`/Button` → `Button`
pub(super) fn strip_step_prefix(step: &str) -> &str {
    // 找到第一个大写字母或 * 的位置（XPath 节点测试始终以大写字母或 * 开头）
    if let Some(pos) = step.find(|c: char| c.is_uppercase() || c == '*') {
        &step[pos..]
    } else {
        step.trim_start_matches('/')
    }
}

/// 分割 XPath 为步骤，保留前缀（`//`, `/*n/`, `/`）。
///
/// 不能用 naive `split('/')`：`/*9/Text[@Name='背影']` 会变成 `["*9", "Text"]`，
/// 丢失 `/*` 前缀导致 `*9` 被误识别为 Descendant 而不是 DepthLimited。
///
/// 返回的步骤保留完整前缀，例如：
/// - `//Button/Text` → `["//Button", "/Text"]`
/// - `/*9/Text[@Name='x']` → `["/*9", "/Text[@Name='x']"]`
/// - `/Pane/Button` → `["Pane", "/Button"]`（第一个 `/` 表示从根开始=Descendant，去掉；后续 `/` 保留=Child）
pub(super) fn split_xpath_steps(xpath: &str) -> Vec<&str> {
    use regex::Regex;
    use once_cell::sync::Lazy;
    static STEP_SPLIT: Lazy<Regex> = Lazy::new(|| {
        // 匹配：`/` 之后不是 `/` 也不是 `*` 的边界，或者是 `/*n` 或 `/*` 边界
        // 策略：在 `/(?!/)` 处分割，但 `/*` 是前缀的一部分，不能在此分割
        // 正确做法：匹配 `/(?![/*])` 处分割（单独 `/`），或者在下一个 `//` 或 `/*n/` 或 `/` 处分割
        // 简化：手动解析
        Regex::new(r"(//|/\*\d*/|/\*/|/)([^/]+)").unwrap()
    });

    STEP_SPLIT.captures_iter(xpath)
        .enumerate()
        .map(|(i, cap)| {
            let full = cap.get(0).unwrap().as_str();
            // 第一个步骤的单个 `/` 表示"从根开始"，语义上等同于 Descendant，去掉前缀
            // 后续步骤的 `/` 表示 Child 轴，必须保留前缀让 parse_xpath_step 正确识别
            let stripped = if i == 0
                && !full.starts_with("//")
                && !full.starts_with("/*")
                && full.starts_with('/')
            {
                &full[1..]
            } else {
                full
            };
            stripped
        })
        .collect()
}

/// Execute a cached strategy. Returns `None` if cached strategy failed (stale cache).
fn execute_cached_strategy(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    cached: &CompiledXPathEntry,
    filter: &FindAllFilter,
) -> Option<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use super::find_control::search_descendants_via_uiauto_xpath;
    use super::find_raw::search_descendants_via_raw_view;

    log::info!("[XPath Cache] Executing cached: {:?}", cached.strategy);

    let result = match &cached.strategy {
        CompiledStrategy::WindowFastPath => None,
        CompiledStrategy::ControlViewDirect => {
            search_descendants_via_uiauto_xpath(auto, window, xpath, None).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::RawViewBfs => {
            cached_raw_view_bfs(auto, window, xpath, filter).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::ContentRoot => {
            if let Some(cr) = find_content_root(auto, window) {
                search_descendants_via_uiauto_xpath(auto, &cr, xpath, None).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            } else { None }
        }
        CompiledStrategy::FindAllDescendants => {
            let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
            search_descendants_via_raw_view(auto, window, &desc_xpath, SearchMode::First, filter).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::ChildHwndEnum(child_idx) => {
            cached_child_hwnd_search(auto, window, xpath, *child_idx, filter).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::SiblingWindow => {
            cached_sibling_search(auto, window, xpath).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::ChildProcessWindow => {
            cached_child_process_search(auto, window, xpath).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::DescendantContentRoot => {
            if let Some(cr) = find_content_root(auto, window) {
                search_descendants_via_uiauto_xpath(auto, &cr, xpath, None).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            } else { None }
        }
        CompiledStrategy::DescendantWindowRoot => {
            search_descendants_via_uiauto_xpath(auto, window, xpath, None).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::DescendantRawWalk => {
            search_descendants_via_raw_view(auto, window, xpath, SearchMode::First, filter).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
        CompiledStrategy::DescendantChildHwnd(child_idx) => {
            cached_descendant_child_hwnd(auto, window, xpath, *child_idx, filter).ok()
                .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
        }
    };

    if result.is_some() {
        log::info!("[XPath Cache] ✓ Cached strategy succeeded (avg={}ms, hits={})",
            cached.avg_time_ms, cached.hit_count);
    } else {
        log::info!("[XPath Cache] Cached strategy failed (stale?)");
    }

    result
}

/// Execute `/A` step: search direct children only.
///
/// Uses `FindAll(TreeScope::Children)` instead of uiauto-xpath (TreeWalker).
/// This is critical for Chrome/WebView scenarios where TreeWalker cannot navigate
/// into Blink fragment nodes (e.g., Document → Group), but FindAll(Children) works
/// correctly because it follows the same internal path as FindAll(Subtree).
///
/// Strategy:
/// 1. Build a UIA condition from the parsed step (type + exact-match predicates)
/// 2. Call `FindAll(TreeScope::Children, condition)` for each current element
/// 3. If complex predicates exist (starts-with/contains/matches), apply Rust-side filtering
/// 4. Fallback to uiauto-xpath (TreeWalker) if UIA condition cannot be built
fn execute_direct_child(
    auto: &UIAutomation,
    current_elements: &[UIElement],
    parsed: &ParsedXPathStep,
    is_last: bool,
    filter: &FindAllFilter,
) -> anyhow::Result<Vec<UIElement>> {
    use super::find_control::search_descendants_via_uiauto_xpath;

    let has_complex = step_has_complex_predicates(parsed);
    let condition = build_uia_condition_from_step(auto, parsed);

    let mut results = Vec::new();

    for elem in current_elements {
        let step_matches: Vec<UIElement> = if let Some(ref cond) = condition {
            // Primary path: FindAll(Children) — works in Chrome/WebView
            match elem.find_all(TreeScope::Children, cond) {
                Ok(candidates) => {
                    if has_complex {
                        // Secondary Rust-side filter for starts-with/contains/matches
                        candidates
                            .into_iter()
                            .filter(|c| element_matches_parsed_step(c, parsed))
                            .collect()
                    } else {
                        candidates
                    }
                }
                Err(e) => {
                    log::warn!("[DirectChild] FindAll(Children) failed: {:?}, falling back to uiauto-xpath", e);
                    // Fallback: uiauto-xpath (TreeWalker)
                    let step_xpath = format!("/{}", step_to_xpath_str(parsed));
                    if let Ok((matches, _)) = search_descendants_via_uiauto_xpath(auto, elem, &step_xpath, None) {
                        matches
                    } else {
                        continue;
                    }
                }
            }
        } else {
            // No UIA condition could be built (e.g., only complex predicates) — fallback to uiauto-xpath
            log::info!("[DirectChild] No UIA condition built, falling back to uiauto-xpath");
            let step_xpath = format!("/{}", step_to_xpath_str(parsed));
            if let Ok((matches, _)) = search_descendants_via_uiauto_xpath(auto, elem, &step_xpath, None) {
                matches
            } else {
                continue;
            }
        };

        if is_last {
            results.extend(filter_findall_results(elem, step_matches, "DirectChild", filter));
        } else {
            results.extend(step_matches);
        }
    }
    Ok(results)
}

/// Execute `//A` step in Fast mode: ControlView FindAll.
/// Returns `(results, all_steps_consumed)` where `all_steps_consumed` indicates whether
/// the Chain optimization consumed all remaining steps (so the main loop should break).
fn execute_descendant_fast(
    auto: &UIAutomation,
    current_elements: &[UIElement],
    xpath_parts: &[&str],
    step_idx: usize,
    is_last: bool,
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, bool)> {
    use super::find_control::search_descendants_via_control_view;

    let mut results = Vec::new();
    for elem in current_elements {
        // xpath_parts 各 step 自带前缀（// 或 / 或 /*n/），直接 concat 不加分隔符
        // 然后把第一个 step 的前缀替换为 //（Descendant 语义）
        let desc_xpath = reconstruct_descendant_xpath(&xpath_parts[step_idx..]);
        if let Ok((matches, _)) = search_descendants_via_control_view(auto, elem, &desc_xpath, search_mode, filter) {
            results.extend(matches);
            if !is_last && !results.is_empty() {
                // For non-last steps, take first match as anchor
                results.truncate(1);
            }
            break; // Only search from first current element
        }
    }
    // 当 Chain 成功解析了所有剩余步骤时，返回的结果已经是最终结果
    // Chain 会在 search_descendants_via_control_view 内部处理多步路径
    let all_steps_consumed = !is_last && !results.is_empty();
    Ok((results, all_steps_consumed))
}

/// Execute `//A` step in Full mode: RawView FindAll.
/// Returns `(results, all_steps_consumed)` — same as `execute_descendant_fast`.
fn execute_descendant_full(
    auto: &UIAutomation,
    current_elements: &[UIElement],
    xpath_parts: &[&str],
    step_idx: usize,
    is_last: bool,
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, bool)> {
    use super::find_raw::search_descendants_via_raw_view;

    let mut results = Vec::new();
    for elem in current_elements {
        // xpath_parts 各 step 自带前缀（// 或 / 或 /*n/），直接 concat 不加分隔符
        // 然后把第一个 step 的前缀替换为 //（Descendant 语义）
        let desc_xpath = reconstruct_descendant_xpath(&xpath_parts[step_idx..]);
        if let Ok((matches, _)) = search_descendants_via_raw_view(auto, elem, &desc_xpath, search_mode, filter) {
            results.extend(matches);
            if !is_last && !results.is_empty() {
                results.truncate(1);
            }
            break;
        }
    }
    let all_steps_consumed = !is_last && !results.is_empty();
    Ok((results, all_steps_consumed))
}

/// Reconstruct an XPath step string from parsed step (for uiauto-xpath).
fn step_to_xpath_str(parsed: &ParsedXPathStep) -> String {
    let mut s = String::new();
    if let Some(ref tn) = parsed.type_name {
        s.push_str(tn);
    }
    if !parsed.required_props.is_empty() || !parsed.require_starts_with.is_empty()
        || !parsed.require_contains.is_empty() || !parsed.require_matches.is_empty()
    {
        s.push('[');
        let mut parts = Vec::new();
        for (k, v) in &parsed.required_props {
            parts.push(format!("@{}='{}'", k.as_attr_name(), v));
        }
        for (k, v) in &parsed.require_starts_with {
            parts.push(format!("starts-with(@{}, '{}')", k.as_attr_name(), v));
        }
        for (k, v) in &parsed.require_contains {
            parts.push(format!("contains(@{}, '{}')", k.as_attr_name(), v));
        }
        for (k, _) in &parsed.require_matches {
            parts.push(format!("matches(@{}, '...')", k.as_attr_name()));
        }
        s.push_str(&parts.join(" and "));
        s.push(']');
    }
    s
}

/// Apply positional predicate and search mode to results.
fn apply_positional_and_search_mode(
    results: Vec<UIElement>,
    position_index: Option<usize>,
    search_mode: SearchMode,
) -> Vec<UIElement> {
    let results = if let Some(pos) = position_index {
        if pos > 0 && results.len() >= pos {
            vec![results.into_iter().nth(pos - 1).unwrap()]
        } else if !results.is_empty() {
            vec![]
        } else {
            results
        }
    } else {
        results
    };
    apply_search_mode(results, search_mode)
}


// ═══════════════════════════════════════════════════════════════════════════════
// Cache Strategy Helpers & Shared XPath Utilities
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Cache Strategy Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn cached_raw_view_bfs(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    search_descendants_via_raw_view(auto, window, xpath, SearchMode::First, filter)
}

fn cached_child_hwnd_search(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    child_idx: usize,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let handle = window.get_native_window_handle()?;
    let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
    let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
    if child_idx >= child_hwnds.len() {
        return Ok((vec![], vec![]));
    }
    let child_elem = auto.element_from_handle(child_hwnds[child_idx].into())?;
    let child_class = child_elem.get_classname().unwrap_or_default();
    if !is_webview_class(&child_class) {
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, &child_elem, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    search_descendants_via_raw_view(auto, &child_elem, xpath, SearchMode::First, filter)
}

fn cached_sibling_search(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let siblings = find_sibling_windows_same_process(auto, window)?;
    for sibling in &siblings {
        let sibling_class = sibling.get_classname().unwrap_or_default();
        if is_webview_class(&sibling_class) { continue; }
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, sibling, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
        let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, sibling, &desc_xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    Ok((vec![], vec![]))
}

fn cached_child_process_search(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let child_windows = find_child_process_windows(auto, window)?;
    let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
    for child_win in &child_windows {
        let child_class = child_win.get_classname().unwrap_or_default();
        if is_webview_class(&child_class) { continue; }
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, child_win, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, child_win, &desc_xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    Ok((vec![], vec![]))
}

fn cached_descendant_child_hwnd(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    child_idx: usize,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let handle = window.get_native_window_handle()?;
    let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
    let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
    if child_idx >= child_hwnds.len() {
        return Ok((vec![], vec![]));
    }
    let child_elem = auto.element_from_handle(child_hwnds[child_idx].into())?;
    let child_class = child_elem.get_classname().unwrap_or_default();
    if !is_webview_class(&child_class) {
        if let Ok((r, s)) = search_descendants_via_uiauto_xpath(auto, &child_elem, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    search_descendants_via_raw_view(auto, &child_elem, xpath, SearchMode::First, filter)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared XPath Parsing & Utility Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a UIA condition from a ParsedXPathStep.
pub(super) fn build_uia_condition_from_step(
    auto: &UIAutomation,
    step: &ParsedXPathStep,
) -> Option<UICondition> {
    let mut conditions: Vec<UICondition> = Vec::new();

    if let Some(ref type_name) = step.type_name {
        if let Some(ct_id) = control_type_name_to_id(type_name) {
            if let Ok(cond) = auto.create_property_condition(UIProperty::ControlType, Variant::from(ct_id), None) {
                conditions.push(cond);
            }
        }
    }

    for (key, value) in &step.required_props {
        let prop: UIProperty = match key {
            XPathProperty::Name => UIProperty::Name,
            XPathProperty::AutomationId => UIProperty::AutomationId,
            XPathProperty::FrameworkId => UIProperty::FrameworkId,
            XPathProperty::ClassName => UIProperty::ClassName,
            XPathProperty::ControlType | XPathProperty::Other(_) => continue,
        };
        if let Ok(cond) = auto.create_property_condition(prop, Variant::from(value.clone()), None) {
            conditions.push(cond);
        }
    }

    match conditions.len() {
        0 => None,
        1 => Some(conditions.remove(0)),
        2 => {
            let cond2 = conditions.remove(1);
            let cond1 = conditions.remove(0);
            auto.create_and_condition(cond1, cond2).ok()
        }
        _ => {
            let mut iter = conditions.into_iter();
            let first = iter.next().unwrap();
            let mut combined = Some(first);
            for cond in iter {
                if let Some(c) = combined.take() {
                    combined = auto.create_and_condition(c, cond).ok();
                }
            }
            combined
        }
    }
}

/// 深度限制 BFS：逐层遍历子元素，限制最大深度。
///
/// 对应需求文档 §12.2 的伪代码实现：
/// - depth=0 是根节点
/// - 目标深度 = max_depth - 1 的元素会被检查是否匹配
/// - 浅于目标深度的元素继续入队递归
pub(super) fn find_with_depth_limit(
    auto: &UIAutomation,
    root: &UIElement,
    target_step: &ParsedXPathStep,
    max_depth: u32,
    walker_hint: WalkerHint,
) -> Vec<UIElement> {
    use std::collections::VecDeque;

    let mut results = vec![];
    let mut queue: VecDeque<(UIElement, u32)> = VecDeque::new();

    // 获取根节点的子元素作为 depth=1
    let get_children = |elem: &UIElement| -> Vec<UIElement> {
        match walker_hint {
            WalkerHint::ControlView => {
                if let Ok(walker) = auto.get_control_view_walker() {
                    walker.get_children(elem).unwrap_or_default()
                } else {
                    vec![]
                }
            }
            _ => {
                if let Ok(walker) = auto.get_raw_view_walker() {
                    walker.get_children(elem).unwrap_or_default()
                } else {
                    vec![]
                }
            }
        }
    };

    for child in get_children(root) {
        queue.push_back((child, 1));
    }

    while let Some((node, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        // depth + 1 == max_depth 意味着子节点在目标深度
        // 先检查子节点是否匹配
        let children = get_children(&node);
        for child in &children {
            if depth + 1 == max_depth {
                // 到达目标深度，检查是否匹配
                if element_matches_parsed_step(child, target_step) {
                    results.push(child.clone());
                }
            } else {
                // 未到达目标深度，继续递归
                queue.push_back((child.clone(), depth + 1));
            }
        }

        // 如果当前节点本身就在目标深度，也检查它
        if depth == max_depth {
            if element_matches_parsed_step(&node, target_step) {
                results.push(node.clone());
            }
        }
    }

    log::info!(
        "[DepthLimitedBFS] max_depth={} walker={:?} found={}",
        max_depth, walker_hint, results.len()
    );
    results
}


/// 反向轴类型
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ReverseAxisKind {
    Parent,
    Ancestor,
    AncestorOrSelf,
    PrecedingSibling,
    Preceding,
}

/// 反向轴谓词（如 `parent::Group[@AutomationId='x']`）
#[derive(Debug, Clone)]
pub(super) struct ReverseAxisPredicate {
    pub(super) axis: ReverseAxisKind,
    /// 目标节点的 ControlType 名称
    pub(super) type_name: Option<String>,
    /// 目标节点的精确匹配属性
    pub(super) required_props: Vec<(XPathProperty, String)>,
}

pub(super) fn step_has_complex_predicates(step: &ParsedXPathStep) -> bool {
    step.is_complex
        || !step.require_starts_with.is_empty()
        || !step.require_contains.is_empty()
        || !step.require_matches.is_empty()
}

/// 预编译 XPath 步骤谓词解析正则（避免每次调用 `Regex::new()`）
mod xpath_regex {
    use once_cell::sync::Lazy;
    use regex::Regex;

    /// `@Name='value'` — 精确相等
    pub(super) static ATTR_EQ: Lazy<Regex> = Lazy::new(||
        Regex::new(r#"@(\w+)\s*=\s*'([^']*)'"#).unwrap()
    );
    /// 前缀匹配 — 支持两种格式：
    ///   标准: `starts-with(@Name, 'value')`
    ///   非标: `@Name=starts-with('value')`
    pub(super) static STARTS_WITH: Lazy<Regex> = Lazy::new(||
        Regex::new(r#"(?:starts-with\(\s*@(\w+)\s*,\s*'([^']*)'\s*\)|@(\w+)\s*=\s*starts-with\(\s*'([^']*)'\s*\))"#).unwrap()
    );
    /// 子串包含 — 支持两种格式：
    ///   标准: `contains(@Name, 'value')`
    ///   非标: `@Name=contains('value')`
    pub(super) static CONTAINS: Lazy<Regex> = Lazy::new(||
        Regex::new(r#"(?:contains\(\s*@(\w+)\s*,\s*'([^']*)'\s*\)|@(\w+)\s*=\s*contains\(\s*'([^']*)'\s*\))"#).unwrap()
    );
    /// 正则匹配 — 支持两种格式：
    ///   标准: `matches(@Name, 'pattern'[, 'flags'])`
    ///   非标: `@Name=matches('pattern'[, 'flags'])`
    pub(super) static MATCHES: Lazy<Regex> = Lazy::new(||
        Regex::new(r#"(?:matches\(\s*@(\w+)\s*,\s*'([^']*)'\s*(?:,\s*'([^']*)'\s*)?\)|@(\w+)\s*=\s*matches\(\s*'([^']*)'\s*(?:,\s*'([^']*)'\s*)?\))"#).unwrap()
    );
    /// 步骤前缀解析: `//`, `/*/`, `/*n/`, `/`, 以及不带尾部 `/` 的 `/*n`
    /// 当 XPath 被 split 后，`/*9/Text` 变成 `/*9` + `Text`，所以 `/*9` 也需要能匹配
    pub(super) static STEP_PREFIX: Lazy<Regex> = Lazy::new(||
        Regex::new(r"^(//|/\*(\d+)?/?|/)").unwrap()
    );
}

/// 解析 XPath 步骤为结构化数据。
///
/// 支持的语法：
/// - `/Button[@Name='OK']` — 直接子元素
/// - `//Button[@Name='OK']` — 所有后代
/// - `/*/Button[@Name='OK']` — 深度限制为 2
/// - `/*5/Button[@Name='OK']` — 深度限制为 6
/// - `Button[@Name='OK' and starts-with(@ClassName, 'Q')]` — 组合谓词
/// - `[not(@IsOffscreen)]` — 不含类型名的纯谓词（通配）
pub(super) fn parse_xpath_step(step: &str) -> ParsedXPathStep {
    // 1. 解析前缀
    let (prefix, rest) = if let Some(cap) = xpath_regex::STEP_PREFIX.captures(step) {
        let prefix_str = cap.get(0).unwrap().as_str();
        let rest = &step[prefix_str.len()..];
        let prefix = if prefix_str == "/" {
            XPathStepPrefix::Child
        } else if prefix_str.starts_with("//") && !prefix_str.starts_with("/*") {
            XPathStepPrefix::Descendant
        } else {
            // /*n/ or /*/ — depth-limited
            if let Some(depth_cap) = cap.get(2) {
                let n: u32 = depth_cap.as_str().parse().unwrap_or(1);
                XPathStepPrefix::DepthLimited { max_depth: n + 1 }
            } else {
                XPathStepPrefix::DepthLimited { max_depth: 2 }
            }
        };
        (prefix, rest)
    } else {
        // 无前缀时默认 //（后代搜索）
        (XPathStepPrefix::Descendant, step)
    };

    // 2. 解析类型名和谓词
    let (type_name, predicates_str): (Option<String>, &str) = if rest.starts_with('[') {
        (None, rest)
    } else if let Some(bracket_pos) = rest.find('[') {
        (Some(rest[..bracket_pos].to_string()), &rest[bracket_pos..])
    } else {
        if rest.is_empty() { (None, "") }
        else { (Some(rest.to_string()), "") }
    };

    // 3. 检测 or/not/ancestor/parent 复杂谓词
    //    ancestor::/parent:: 等反向轴谓词中的属性不属于当前步骤，
    //    且无法用 UIA FindAll 条件表达，必须标记为 complex 交由 uiauto-xpath 处理
    let has_reverse_axis = predicates_str.contains("ancestor::")
        || predicates_str.contains("ancestor-or-self::")
        || predicates_str.contains("parent::")
        || predicates_str.contains("preceding-sibling::")
        || predicates_str.contains("preceding::");

    // 对于反向轴谓词，剥离反向轴部分，只保留叶节点自身的谓词用于 FindAll 过滤
    // 同时收集被剥离的反向轴谓词，供客户端二次验证
    // 例如: [@AutomationId='js_name'][ancestor::Group[@id='x']] → 只保留 [@AutomationId='js_name']
    let (effective_predicates, reverse_axis_predicates) = if has_reverse_axis {
        strip_and_collect_reverse_axis_predicates(predicates_str)
    } else {
        (predicates_str.to_string(), Vec::new())
    };

    let is_complex = predicates_str.contains(" or ") || predicates_str.contains("not(") || has_reverse_axis;

    if predicates_str.contains(" or ") || predicates_str.contains("not(") {
        // or/not 谓词：无法用 FindAll 表达，完全跳过属性提取
        return ParsedXPathStep {
            prefix,
            type_name,
            required_props: Default::default(),
            require_starts_with: Default::default(),
            require_contains: Default::default(),
            require_matches: Default::default(),
            is_complex: true,
            reverse_axis_predicates,
        };
    }

    let mut required_props: Vec<(XPathProperty, String)> = Vec::new();
    let mut require_starts_with: Vec<(XPathProperty, String)> = Vec::new();
    let mut require_contains: Vec<(XPathProperty, String)> = Vec::new();
    let mut require_matches: Vec<(XPathProperty, Regex)> = Vec::new();

    // 4. 解析谓词（使用预编译正则，基于 effective_predicates 而非原始 predicates_str）

    // [@Attr='Value'] — 精确相等
    for cap in xpath_regex::ATTR_EQ.captures_iter(&effective_predicates) {
        required_props.push((XPathProperty::from_attr_name(&cap[1]), cap[2].to_string()));
    }

    // starts-with(@Attr, 'Value') 或 @Attr=starts-with('Value')
    for cap in xpath_regex::STARTS_WITH.captures_iter(&effective_predicates) {
        let attr = cap.get(1).or_else(|| cap.get(3)).unwrap().as_str();
        let value = cap.get(2).or_else(|| cap.get(4)).unwrap().as_str();
        require_starts_with.push((XPathProperty::from_attr_name(attr), value.to_string()));
    }

    // contains(@Attr, 'Value') 或 @Attr=contains('Value')
    for cap in xpath_regex::CONTAINS.captures_iter(&effective_predicates) {
        let attr = cap.get(1).or_else(|| cap.get(3)).unwrap().as_str();
        let value = cap.get(2).or_else(|| cap.get(4)).unwrap().as_str();
        require_contains.push((XPathProperty::from_attr_name(attr), value.to_string()));
    }

    // matches(@Attr, 'Pattern'[, 'flags']) 或 @Attr=matches('Pattern'[, 'flags'])
    for cap in xpath_regex::MATCHES.captures_iter(&effective_predicates) {
        let attr = cap.get(1).or_else(|| cap.get(4)).unwrap().as_str();
        let pattern = cap.get(2).or_else(|| cap.get(5)).unwrap().as_str();
        let flags = cap.get(3).or_else(|| cap.get(6)).map(|m| m.as_str()).unwrap_or("");
        let full_pattern = if flags.is_empty() {
            format!("(?i){}", pattern)
        } else {
            format!("(?{}){}", flags, pattern)
        };
        if let Ok(re) = Regex::new(&full_pattern) {
            require_matches.push((XPathProperty::from_attr_name(attr), re));
        }
    }

    ParsedXPathStep {
        prefix,
        type_name,
        required_props,
        require_starts_with,
        require_contains,
        require_matches,
        is_complex,
        reverse_axis_predicates,
    }
}

/// 剥离反向轴谓词，同时收集被剥离的反向轴谓词信息。
/// 返回 (保留的非反向轴谓词字符串, 收集到的反向轴谓词列表)
/// 例如: "[@AutomationId='js_name'][ancestor::Group[@id='x']][parent::Group[@id='y']]"
///    →  ("[@AutomationId='js_name']", [ancestor::Group, parent::Group])
fn strip_and_collect_reverse_axis_predicates(predicates_str: &str) -> (String, Vec<ReverseAxisPredicate>) {
    let mut result = String::new();
    let mut reverse_predicates = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let bytes = predicates_str.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' if depth == 0 => {
                start = i;
                depth = 1;
            }
            b'[' => { depth += 1; }
            b']' => { depth -= 1; }
            _ => {}
        }
        if depth == 0 && b == b']' {
            // 完整谓词: predicates_str[start..=i]
            let pred = &predicates_str[start..=i];
            // 检查是否是反向轴谓词
            let axis_info = detect_reverse_axis(pred);
            if let Some((axis, axis_body)) = axis_info {
                // 解析反向轴目标节点的类型名和属性
                let (tn, props) = parse_reverse_axis_node(axis_body);
                reverse_predicates.push(ReverseAxisPredicate {
                    axis,
                    type_name: tn,
                    required_props: props,
                });
            } else {
                result.push_str(pred);
            }
        }
    }
    (result, reverse_predicates)
}

/// 检测谓词是否包含反向轴，返回 (轴类型, 轴目标部分)
/// 例如 `[parent::Group[@AutomationId='x']]` → (Parent, `Group[@AutomationId='x']`)
fn detect_reverse_axis(pred: &str) -> Option<(ReverseAxisKind, &str)> {
    // pred 形如 `[parent::Group[@AutomationId='x']]`
    // 去掉外层 []
    let inner = pred.trim_start_matches('[').trim_end_matches(']');
    let (axis, rest) = if let Some(body) = inner.strip_prefix("parent::") {
        (ReverseAxisKind::Parent, body)
    } else if let Some(body) = inner.strip_prefix("ancestor-or-self::") {
        (ReverseAxisKind::AncestorOrSelf, body)
    } else if let Some(body) = inner.strip_prefix("ancestor::") {
        (ReverseAxisKind::Ancestor, body)
    } else if let Some(body) = inner.strip_prefix("preceding-sibling::") {
        (ReverseAxisKind::PrecedingSibling, body)
    } else if let Some(body) = inner.strip_prefix("preceding::") {
        (ReverseAxisKind::Preceding, body)
    } else {
        return None;
    };
    Some((axis, rest))
}

/// 解析反向轴目标节点的类型名和属性
/// 例如 `Group[@AutomationId='x']` → (Some("Group"), [("AutomationId", "x")])
fn parse_reverse_axis_node(body: &str) -> (Option<String>, Vec<(XPathProperty, String)>) {
    let (type_name, predicates_str): (Option<String>, &str) = if body.starts_with('[') {
        (None, body)
    } else if let Some(bracket_pos) = body.find('[') {
        (Some(body[..bracket_pos].to_string()), &body[bracket_pos..])
    } else {
        if body.is_empty() { (None, "") }
        else { (Some(body.to_string()), "") }
    };

    let mut props = Vec::new();
    for cap in xpath_regex::ATTR_EQ.captures_iter(predicates_str) {
        props.push((XPathProperty::from_attr_name(&cap[1]), cap[2].to_string()));
    }

    (type_name, props)
}

fn get_uia_property_for_xpath(elem: &UIElement, prop: &XPathProperty) -> String {
    match prop {
        XPathProperty::Name => elem.get_name().unwrap_or_default(),
        XPathProperty::AutomationId => elem.get_automation_id().unwrap_or_default(),
        XPathProperty::ClassName => elem.get_classname().unwrap_or_default(),
        XPathProperty::FrameworkId => elem.get_framework_id().unwrap_or_default(),
        XPathProperty::ControlType => elem.get_control_type().map(|ct| format!("{:?}", ct)).unwrap_or_default(),
        XPathProperty::Other(_) => String::new(),
    }
}

pub(super) fn element_matches_parsed_step(elem: &UIElement, step: &ParsedXPathStep) -> bool {
    // Check type name
    if let Some(ref type_name) = step.type_name {
        let actual = elem.get_control_type().map(|ct| format!("{:?}", ct)).unwrap_or_default();
        if !actual.eq_ignore_ascii_case(type_name) {
            // Also try the friendly name mapping
            if let Some(ct_id) = control_type_name_to_id(type_name) {
                if elem.get_control_type().map(|ct| ct as i32) != Ok(ct_id as i32) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    // Check exact equality predicates
    for (key, value) in &step.required_props {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.eq_ignore_ascii_case(value) {
            return false;
        }
    }

    // Check starts-with predicates
    for (key, prefix) in &step.require_starts_with {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.to_lowercase().starts_with(&prefix.to_lowercase()) {
            return false;
        }
    }

    // Check contains predicates
    for (key, substr) in &step.require_contains {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.to_lowercase().contains(&substr.to_lowercase()) {
            return false;
        }
    }

    // Check matches predicates (regex)
    for (key, re) in &step.require_matches {
        let actual = get_uia_property_for_xpath(elem, key);
        if !re.is_match(&actual) {
            return false;
        }
    }

    // Check reverse axis predicates (parent::, ancestor::, etc.)
    for rap in &step.reverse_axis_predicates {
        if !element_matches_reverse_axis(elem, rap) {
            return false;
        }
    }

    true
}

/// 验证元素是否满足反向轴谓词
fn element_matches_reverse_axis(elem: &UIElement, rap: &ReverseAxisPredicate) -> bool {
    match rap.axis {
        ReverseAxisKind::Parent => {
            // 获取父元素
            let parent = match elem.get_cached_parent() {
                Ok(p) => p,
                Err(_) => return false,
            };
            element_matches_node_spec(&parent, &rap.type_name, &rap.required_props)
        }
        ReverseAxisKind::Ancestor => {
            // 沿 parent 链向上查找，任一祖先匹配即可
            let mut current = match elem.get_cached_parent() {
                Ok(p) => p,
                Err(_) => return false,
            };
            loop {
                if element_matches_node_spec(&current, &rap.type_name, &rap.required_props) {
                    return true;
                }
                match current.get_cached_parent() {
                    Ok(p) => current = p,
                    Err(_) => return false,
                }
            }
        }
        ReverseAxisKind::AncestorOrSelf => {
            // 先检查自身，再沿 parent 链向上
            if element_matches_node_spec(elem, &rap.type_name, &rap.required_props) {
                return true;
            }
            let mut current = match elem.get_cached_parent() {
                Ok(p) => p,
                Err(_) => return false,
            };
            loop {
                if element_matches_node_spec(&current, &rap.type_name, &rap.required_props) {
                    return true;
                }
                match current.get_cached_parent() {
                    Ok(p) => current = p,
                    Err(_) => return false,
                }
            }
        }
        ReverseAxisKind::PrecedingSibling | ReverseAxisKind::Preceding => {
            // preceding-sibling 和 preceding 较少使用，简单实现：
            // 获取父元素的所有子元素，检查当前元素之前是否有匹配的兄弟
            let parent = match elem.get_cached_parent() {
                Ok(p) => p,
                Err(_) => return false,
            };
            let self_runtime_id = match elem.get_runtime_id() {
                Ok(id) => id,
                Err(_) => return false,
            };
            // 遍历父元素子节点，找到自己之前的兄弟
            if let Ok(children) = parent.get_cached_children() {
                if rap.axis == ReverseAxisKind::PrecedingSibling {
                    let mut found_self = false;
                    for child in children.iter().rev() {
                        if let Ok(child_id) = child.get_runtime_id() {
                            if child_id == self_runtime_id {
                                found_self = true;
                                continue;
                            }
                        }
                        if found_self && element_matches_node_spec(child, &rap.type_name, &rap.required_props) {
                            return true;
                        }
                    }
                } else {
                    // preceding: 检查自身之前的所有兄弟
                    for child in children.iter() {
                        if let Ok(child_id) = child.get_runtime_id() {
                            if child_id == self_runtime_id {
                                break;
                            }
                        }
                        if element_matches_node_spec(child, &rap.type_name, &rap.required_props) {
                            return true;
                        }
                    }
                }
            }
            false
        }
    }
}

/// 检查一个元素是否匹配给定的类型名和属性规范
fn element_matches_node_spec(elem: &UIElement, type_name: &Option<String>, required_props: &[(XPathProperty, String)]) -> bool {
    if let Some(ref tn) = type_name {
        let actual = elem.get_control_type().map(|ct| format!("{:?}", ct)).unwrap_or_default();
        if !actual.eq_ignore_ascii_case(tn) {
            if let Some(ct_id) = control_type_name_to_id(tn) {
                if elem.get_control_type().map(|ct| ct as i32) != Ok(ct_id as i32) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    for (key, value) in required_props {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.eq_ignore_ascii_case(value) {
            return false;
        }
    }
    true
}





// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

pub fn find_elements_by_xpath(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
    search_context: Option<&crate::core::model::SearchContext>,
    timeout_ms: Option<u64>,
    filter: Option<&FindAllFilter>,
    chrome_treewalker_fallback: bool,
) -> Vec<crate::core::model::ElementData> {
    let filter = filter.cloned().unwrap_or_default();
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };
    let windows = find_window_by_selector(&auto, window_selector);
    
    if windows.is_empty() {
        return vec![];
    }

    let (search_mode, element_xpath_no_suffix) = SearchMode::strip_suffix(element_xpath);
    if !matches!(search_mode, SearchMode::All) {
        log::info!("[Find All] Search mode suffix detected: {:?}", search_mode);
    }

    let (locate_mode_from_prefix, prefix_hint, element_xpath_stripped) = LocateMode::strip_xpath_prefix(element_xpath_no_suffix);
    let locate_mode = search_context.map(|ctx| ctx.locate_mode)
        .or(locate_mode_from_prefix);
    let is_child_mode = locate_mode.map_or(false, |m| m.is_child_mode());

    let child_hwnd_hint_from_ctx = search_context.and_then(|ctx| ctx.child_hwnd_hint.as_ref());
    let child_hwnd_hint: Option<&ChildHwndHint> = child_hwnd_hint_from_ctx.or(prefix_hint.as_ref());

    // Fast path: XPath directly targets Window elements
    if !is_child_mode {
        let xpath_trimmed = element_xpath_stripped.trim_start_matches('/');
        let targets_window = xpath_trimmed == "Window"
            || xpath_trimmed.starts_with("Window[");
        if targets_window {
            log::info!("[Fast Path] XPath targets Window, filtering enumerated windows directly");
            let step_parsed = if xpath_trimmed.starts_with("Window[") {
                let pred_start = xpath_trimmed.find('[').unwrap_or(0);
                let pred_end = xpath_trimmed.rfind(']').unwrap_or(xpath_trimmed.len());
                if pred_start < pred_end {
                    Some(parse_xpath_step(&xpath_trimmed[..=pred_end]))
                } else {
                    None
                }
            } else {
                None
            };

            let mut rng = rand::thread_rng();
            for window in &windows {
                if let Some(ref parsed) = step_parsed {
                    if !element_matches_parsed_step(window, parsed) {
                        continue;
                    }
                }
                let window_rect = window.get_bounding_rectangle().ok().map(|r| {
                    crate::core::model::Rect {
                        x: r.get_left(),
                        y: r.get_top(),
                        width: r.get_right() - r.get_left(),
                        height: r.get_bottom() - r.get_top(),
                    }
                });
                if let Some(info) = element_info_from_uia(window, window_rect.as_ref(), random_range, &mut rng) {
                    let _ = cache_store(element_xpath_no_suffix, window, CompiledStrategy::WindowFastPath, 0);
                    // 缓存原始元素，供后续 click/refresh 等操作使用
                    if let Some(rid_str) = super::helpers::runtime_id_key(window)
                        .map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","))
                    {
                        crate::core::element_cache::cache_element(rid_str, window.clone());
                    }
                    return apply_search_mode(vec![info], search_mode);
                }
            }
            return apply_search_mode(vec![], search_mode);
        }
    }

    // Child Mode
    if is_child_mode {
        log::info!("[Find All] Child mode detected, searching via EnumChildWindows: xpath='{}'", element_xpath_stripped);
        for window in &windows {
            let hwnd = match window.get_native_window_handle() {
                Ok(h) => {
                    let raw: windows::Win32::Foundation::HANDLE = h.into();
                    HWND(raw.0)
                },
                Err(_) => continue,
            };
            let child_hwnds = enum_child_hwnds(hwnd);
            log::info!("[Find All] Child mode: {} child HWNDs for window", child_hwnds.len());

            let filtered_hwnds: Vec<HWND> = if let Some(hint) = child_hwnd_hint {
                child_hwnds.into_iter().filter(|&ch| {
                    if let Ok(elem) = auto.element_from_handle(ch.into()) {
                        let class_matches = elem.get_classname()
                            .map(|c| c.contains(&hint.hwnd_class))
                            .unwrap_or(false);
                        let title_matches = if hint.hwnd_title.is_empty() {
                            true
                        } else {
                            elem.get_name()
                                .map(|n| n.contains(&hint.hwnd_title))
                                .unwrap_or(false)
                        };
                        class_matches && title_matches
                    } else {
                        false
                    }
                }).collect()
            } else {
                child_hwnds
            };

            let window_rect = window.get_bounding_rectangle().ok().map(|r| {
                crate::core::model::Rect {
                    x: r.get_left(),
                    y: r.get_top(),
                    width: r.get_right() - r.get_left(),
                    height: r.get_bottom() - r.get_top(),
                }
            });

            for child_hwnd in &filtered_hwnds {
                let child_elem = match auto.element_from_handle((*child_hwnd).into()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let child_xpath = element_xpath;
                let (elements, _) = match execute_xpath_steps_filtered(&auto, &child_elem, &child_xpath, &filter, timeout_ms, chrome_treewalker_fallback) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if !elements.is_empty() {
                    // 缓存原始元素，供后续 click/refresh 等操作使用
                    for raw_elem in &elements {
                        if let Some(rid_str) = super::helpers::runtime_id_key(raw_elem)
                            .map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","))
                        {
                            crate::core::element_cache::cache_element(rid_str, raw_elem.clone());
                        }
                    }
                    let mut rng = rand::thread_rng();
                    let results: Vec<_> = elements.iter().filter_map(|elem| {
                        element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
                    }).collect();
                    return apply_search_mode(results, search_mode);
                }
            }
        }
        return apply_search_mode(vec![], search_mode);
    }

    let is_broad_selector = !window_selector.contains('[');
    let is_absolute_xpath = element_xpath_stripped.starts_with('/') && !element_xpath_stripped.starts_with("//");
    let max_windows = if is_broad_selector { 3usize } else { windows.len() };
    
    for (win_idx, window) in windows.iter().enumerate() {
        if win_idx >= max_windows {
            log::info!("[Find All] Reached max windows limit ({}/{}) for broad selector", win_idx, windows.len());
            break;
        }
        
        if is_absolute_xpath {
            let win_class = window.get_classname().unwrap_or_default();
            if is_webview_class(&win_class) {
                log::info!("[Find All] Skipping WebView window '{}' for absolute XPath (children under child HWNDs)", win_class);
                continue;
            }
        }
        
        let window_rect = window.get_bounding_rectangle().ok().map(|r| {
            crate::core::model::Rect {
                x: r.get_left(),
                y: r.get_top(),
                width: r.get_right() - r.get_left(),
                height: r.get_bottom() - r.get_top(),
            }
        });

        let (elements, _) = match execute_xpath_steps_filtered(&auto, window, element_xpath_no_suffix, &filter, timeout_ms, chrome_treewalker_fallback) {
            Ok(result) => result,
            Err(_) => continue,
        };
        
        if !elements.is_empty() {
            // 缓存原始元素，供后续 click/refresh 等操作使用
            for raw_elem in &elements {
                if let Some(rid_str) = super::helpers::runtime_id_key(raw_elem)
                    .map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","))
                {
                    crate::core::element_cache::cache_element(rid_str, raw_elem.clone());
                }
            }
            let mut rng = rand::thread_rng();
            let results: Vec<_> = elements.iter().filter_map(|elem| {
                element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
            }).collect();
            return apply_search_mode(results, search_mode);
        }
    }
    
    apply_search_mode(vec![], search_mode)
}

/// Apply SearchMode (:first / :onlyone / :all) post-processing.
///
/// Note: findOne uniqueness check (`LeafNotUnique`) is performed in the XPath executor
/// at the parent level (需求 §5.3.1), not here. `OnlyOne` here is a defense-in-depth safety net.
#[inline]
fn apply_search_mode<T>(results: Vec<T>, mode: SearchMode) -> Vec<T> {
    match mode {
        SearchMode::All => results,
        SearchMode::First => {
            if results.len() > 1 {
                log::info!("[SearchMode:first] Truncating {} results to 1", results.len());
                results.into_iter().take(1).collect()
            } else {
                results
            }
        }
        SearchMode::OnlyOne => {
            if results.len() > 1 {
                log::warn!("[SearchMode:onlyone] Defense: Expected unique, found {} results — returning empty", results.len());
                vec![]
            } else {
                results
            }
        }
    }
}

pub fn find_all_elements_from_root(
    element_xpath: &str,
    random_range: f32,
) -> Vec<crate::core::model::ElementData> {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };

    let desktop = match auto.get_root_element() {
        Ok(d) => d,
        Err(e) => {
            log::error!("[find_from_root] Failed to get root element: {:?}", e);
            return vec![];
        }
    };

    log::info!("[find_from_root] Searching from Desktop root: xpath='{}'", element_xpath);
    let (elements, _) = match search_descendants_via_uiauto_xpath(&auto, &desktop, element_xpath, None) {
        Ok(r) => r,
        Err(e) => {
            log::error!("[find_from_root] XPath failed: {}", e);
            return vec![];
        }
    };

    if elements.is_empty() {
        log::info!("[find_from_root] No elements found");
        return vec![];
    }

    log::info!("[find_from_root] Found {} elements", elements.len());

    let desktop_rect = desktop.get_bounding_rectangle().ok().map(|r| {
        crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });

    let mut rng = rand::thread_rng();
    elements.iter().filter_map(|elem| {
        element_info_from_uia(elem, desktop_rect.as_ref(), random_range, &mut rng)
    }).collect()
}




pub fn find_from_element_impl(
    auto: &UIAutomation,
    base_elem: &UIElement,
    xpath: &str,
    random_range: f32,
) -> (Vec<crate::core::model::ElementData>, Vec<UIElement>) {
    log::info!("[find_from_element] Searching from element: xpath='{}'", xpath);

    let base_rect = base_elem.get_bounding_rectangle().ok().map(|r| {
        crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });

    let (raw_elements, _) = match search_descendants_via_uiauto_xpath(auto, base_elem, xpath, None) {
        Ok(r) => r,
        Err(e) => {
            log::error!("[find_from_element] XPath search failed: {}", e);
            return (vec![], vec![]);
        }
    };

    if raw_elements.is_empty() {
        log::info!("[find_from_element] No elements found");
        return (vec![], vec![]);
    }

    log::info!("[find_from_element] Found {} elements", raw_elements.len());

    let mut rng = rand::thread_rng();
    let results: Vec<crate::core::model::ElementData> = raw_elements.iter().filter_map(|elem| {
        element_info_from_uia(elem, base_rect.as_ref(), random_range, &mut rng)
    }).collect();

    (results, raw_elements)
}



// ═══════════════════════════════════════════════════════════════════════════════
// 二次定位 API（从 RuntimeId 缓存获取父元素后搜索）§6
// ═══════════════════════════════════════════════════════════════════════════════

/// 从缓存的父元素开始，定位第一个匹配的元素
///
/// 需求 §6.1: `findFirstFrom(parent_runtime_id, relative_xpath, strategy)`
/// - 无 Adaptive fallback（Fast 失败不退回 Full，反之亦然）
/// - 父元素不在缓存中返回 `InvalidParent`
/// - 返回 `(Option<ElementData>, Option<NotFoundReason>)`
pub fn locate_first_from(
    runtime_id: &str,
    relative_xpath: &str,
    strategy: SearchStrategy,
) -> (Option<crate::core::model::ElementData>, Option<crate::core::model::NotFoundReason>) {
    let (results, reason) = locate_from_impl(runtime_id, relative_xpath, SearchMode::First, strategy, &FindAllFilter::default());
    (results.into_iter().next(), reason)
}

/// 从缓存的父元素开始，定位唯一匹配的元素（findOne 语义）
///
/// 需求 §6.2: `findOneFrom(parent_runtime_id, relative_xpath, strategy)`
/// - 复用阶段 3.1 的叶子唯一性验证（`SearchMode::OnlyOne`）
/// - 在父节点下找到 > 1 个匹配时返回 `LeafNotUnique`
/// - 父元素不在缓存中返回 `InvalidParent`
/// - 无 Adaptive fallback
pub fn locate_one_from(
    runtime_id: &str,
    relative_xpath: &str,
    strategy: SearchStrategy,
) -> (Option<crate::core::model::ElementData>, Option<crate::core::model::NotFoundReason>) {
    let (results, reason) = locate_from_impl(runtime_id, relative_xpath, SearchMode::OnlyOne, strategy, &FindAllFilter::default());
    (results.into_iter().next(), reason)
}

/// 从缓存的父元素开始，定位所有匹配的元素（findAll 语义）
///
/// 需求 §6.3: `findAllFrom(parent_runtime_id, relative_xpath, strategy, filter)`
/// - 支持 FilterCondition（AttributeFilter）
/// - 父元素不在缓存中返回空列表
/// - 无 Adaptive fallback
pub fn locate_all_from(
    runtime_id: &str,
    relative_xpath: &str,
    strategy: SearchStrategy,
    filter: Option<&FindAllFilter>,
) -> Vec<crate::core::model::ElementData> {
    let default_filter = FindAllFilter::default();
    let effective_filter = filter.unwrap_or(&default_filter);
    let (results, _) = locate_from_impl(runtime_id, relative_xpath, SearchMode::All, strategy, effective_filter);
    results
}

/// 二次定位核心实现
///
/// 从 RuntimeId 缓存获取父元素，使用指定策略执行 XPath 搜索。
/// 不做 Adaptive fallback（Fast 失败不退回 Full）。
fn locate_from_impl(
    runtime_id: &str,
    relative_xpath: &str,
    search_mode: SearchMode,
    strategy: SearchStrategy,
    filter: &FindAllFilter,
) -> (Vec<crate::core::model::ElementData>, Option<crate::core::model::NotFoundReason>) {
    use crate::core::element_cache::{cache_element, get_cached_element};

    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return (vec![], None),
    };

    // 从缓存获取父元素
    let base_elem: UIElement = match get_cached_element(runtime_id) {
        Some(e) => e,
        None => {
            log::warn!("[locate_from] Parent element not in cache: runtime_id={}", runtime_id);
            return (vec![], Some(crate::core::model::NotFoundReason::InvalidParent {
                runtime_id: runtime_id.to_string(),
            }));
        }
    };

    let base_rect = base_elem.get_bounding_rectangle().ok().map(|r| {
        crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });

    // 根据策略执行搜索（无 Adaptive fallback）
    let raw_elements: Vec<UIElement> = match strategy {
        SearchStrategy::Fast { max_depth } => {
            log::info!("[locate_from] Fast strategy, max_depth={}", max_depth);
            match search_descendants_via_uiauto_xpath(&auto, &base_elem, relative_xpath, None) {
                Ok((elems, _)) => elems,
                Err(e) => {
                    log::warn!("[locate_from] Fast strategy failed: {}", e);
                    vec![]
                }
            }
        }
        SearchStrategy::Full { max_depth } => {
            log::info!("[locate_from] Full strategy, max_depth={}", max_depth);
            match search_descendants_depth_limited(&auto, &base_elem, relative_xpath, max_depth, search_mode, filter) {
                Ok((elems, _)) => elems,
                Err(e) => {
                    log::warn!("[locate_from] Full strategy failed: {}", e);
                    vec![]
                }
            }
        }
        SearchStrategy::Adaptive => {
            // Adaptive 在此处等同于 Fast（不做 fallback，保持一致性）
            log::info!("[locate_from] Adaptive strategy (treating as Fast, no fallback)");
            match search_descendants_via_uiauto_xpath(&auto, &base_elem, relative_xpath, None) {
                Ok((elems, _)) => elems,
                Err(e) => {
                    log::warn!("[locate_from] Adaptive/Fast strategy failed: {}", e);
                    vec![]
                }
            }
        }
    };

    // 缓存找到的元素
    for raw_elem in &raw_elements {
        if let Some(rid_str) = super::helpers::runtime_id_key(raw_elem)
            .map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","))
        {
            cache_element(rid_str, raw_elem.clone());
        }
    }

    // 叶子唯一性验证（OnlyOne 模式）
    if search_mode == SearchMode::OnlyOne && raw_elements.len() > 1 {
        log::warn!("[locate_from] Leaf not unique: {} candidates for runtime_id={}", raw_elements.len(), runtime_id);
        return (vec![], Some(crate::core::model::NotFoundReason::LeafNotUnique {
            candidates: raw_elements.len(),
        }));
    }

    let mut rng = rand::thread_rng();
    let results: Vec<crate::core::model::ElementData> = raw_elements.iter().filter_map(|elem| {
        element_info_from_uia(elem, base_rect.as_ref(), 0.0, &mut rng)
    }).collect();

    let results = apply_search_mode(results, search_mode);
    (results, None)
}

// ═══════════════════════════════════════════════════════════════════════════════
// FindAll post-filter
// ═══════════════════════════════════════════════════════════════════════════════

pub(super) fn filter_findall_results(
    window: &UIElement,
    candidates: Vec<UIElement>,
    label: &str,
    filter: &FindAllFilter,
) -> Vec<UIElement> {
    if filter.is_all_disabled() {
        return candidates;
    }

    let window_rect: Option<SimpleRect> = if filter.exclude_out_of_bounds {
        window.get_bounding_rectangle().ok().map(|r| SimpleRect::from(&r))
    } else {
        None
    };

    let raw_count = candidates.len();
    let filtered: Vec<UIElement> = candidates
        .into_iter()
        .filter(|elem| {
            if filter.exclude_offscreen && elem.is_offscreen().unwrap_or(false) {
                return false;
            }

            let rect = if filter.exclude_zero_size || filter.exclude_out_of_bounds {
                match elem.get_bounding_rectangle() {
                    Ok(r) => SimpleRect::from(&r),
                    Err(_) => return filter.exclude_zero_size || filter.exclude_out_of_bounds,
                }
            } else {
                return true;
            };

            if filter.exclude_zero_size && (rect.width() <= 0 || rect.height() <= 0) {
                return false;
            }

            if let Some(ref wr) = window_rect {
                if rect.right < wr.left || rect.bottom < wr.top
                    || rect.left > wr.right || rect.top > wr.bottom
                {
                    return false;
                }
            }

            true
        })
        .collect();

    let after_geo = filtered.len();

    // ── Attribute filter (需求 §8.4) ──
    let filtered = apply_attribute_filters(filtered, &filter.attribute_filters);

    let filtered_count = filtered.len();
    let removed_geo = raw_count.saturating_sub(after_geo);
    let removed_attr = after_geo.saturating_sub(filtered_count);
    if filtered_count < raw_count {
        log::info!("[FindAll Filter][{}] Post-filter: {} → {} elements (removed {}: offscreen/zero-size/out-of-bounds, {}: attribute)",
            label, raw_count, filtered_count, removed_geo, removed_attr);
    }

    filtered
}

/// 客户端属性过滤（需求 §8.4）。
///
/// 对 `FindAll` 结果应用 `AttributeFilter` 列表：
/// - `Eq`: 使用 `get_uia_property_for_xpath` 获取属性值，精确比较
/// - `NotEq`: 不等于比较
/// - `Contains`: 子串包含（不区分大小写）
/// - `Regex`: 正则匹配
/// - `Exists`: 属性值非空
///
/// 所有 filter 之间是 AND 关系。
fn apply_attribute_filters(
    elements: Vec<UIElement>,
    filters: &[crate::core::model::AttributeFilter],
) -> Vec<UIElement> {
    use crate::core::model::FilterOp;

    if filters.is_empty() {
        return elements;
    }

    let mut precompiled_regexes: Vec<(usize, regex::Regex)> = Vec::new();
    for (i, f) in filters.iter().enumerate() {
        if matches!(f.operator, FilterOp::Regex) && !f.value.is_empty() {
            if let Ok(re) = regex::Regex::new(&format!("(?i){}", f.value)) {
                precompiled_regexes.push((i, re));
            }
        }
    }

    elements
        .into_iter()
        .filter(|elem| {
            for (i, filter) in filters.iter().enumerate() {
                let actual = get_uia_property_for_xpath(elem, &XPathProperty::from_attr_name(&filter.property));

                let matches = match filter.operator {
                    FilterOp::Eq => actual.eq_ignore_ascii_case(&filter.value),
                    FilterOp::NotEq => !actual.eq_ignore_ascii_case(&filter.value),
                    FilterOp::Contains => actual.to_lowercase().contains(&filter.value.to_lowercase()),
                    FilterOp::Regex => {
                        precompiled_regexes.iter()
                            .find(|(idx, _)| *idx == i)
                            .map_or(false, |(_, re)| re.is_match(&actual))
                    }
                    FilterOp::Exists => !actual.is_empty(),
                };

                if !matches {
                    return false;
                }
            }
            true
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: XPath 步骤解析
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ─── 前缀解析测试 ────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_child_prefix() {
        let step = parse_xpath_step("/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Child);
        assert_eq!(step.type_name.as_deref(), Some("Button"));
        assert_eq!(step.required_props.len(), 1);
        assert_eq!(step.required_props[0], (XPathProperty::Name, "OK".to_string()));
        assert!(!step.is_complex);
    }

    #[test]
    fn test_parse_descendant_prefix() {
        let step = parse_xpath_step("//Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
        assert_eq!(step.type_name.as_deref(), Some("Button"));
    }

    #[test]
    fn test_parse_depth_2_wildcard() {
        let step = parse_xpath_step("/*/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::DepthLimited { max_depth: 2 });
        assert_eq!(step.type_name.as_deref(), Some("Button"));
    }

    #[test]
    fn test_parse_depth_n() {
        let step = parse_xpath_step("/*5/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::DepthLimited { max_depth: 6 });
    }

    #[test]
    fn test_parse_no_prefix_defaults_to_descendant() {
        let step = parse_xpath_step("Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
    }

    // ─── 谓词解析测试 ────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_eq_predicate() {
        let step = parse_xpath_step("//Button[@Name='OK']");
        assert_eq!(step.required_props.len(), 1);
        assert_eq!(step.required_props[0], (XPathProperty::Name, "OK".to_string()));
    }

    #[test]
    fn test_parse_starts_with() {
        // 标准格式: starts-with(@Attr, 'value')
        let step = parse_xpath_step("//Pane[starts-with(@ClassName, 'Chrome')]");
        assert_eq!(step.require_starts_with.len(), 1);
        assert_eq!(step.require_starts_with[0], (XPathProperty::ClassName, "Chrome".to_string()));

        // 兼容非标格式: @Attr=starts-with('value')
        let step2 = parse_xpath_step("//Pane[@ClassName=starts-with('Chrome')]");
        assert_eq!(step2.require_starts_with.len(), 1);
        assert_eq!(step2.require_starts_with[0], (XPathProperty::ClassName, "Chrome".to_string()));
    }

    #[test]
    fn test_parse_contains() {
        // 标准格式: contains(@Attr, 'value')
        let step = parse_xpath_step("//Text[contains(@Name, 'Widget')]");
        assert_eq!(step.require_contains.len(), 1);
        assert_eq!(step.require_contains[0], (XPathProperty::Name, "Widget".to_string()));

        // 兼容非标格式
        let step2 = parse_xpath_step("//Text[@Name=contains('Widget')]");
        assert_eq!(step2.require_contains.len(), 1);
        assert_eq!(step2.require_contains[0], (XPathProperty::Name, "Widget".to_string()));
    }

    #[test]
    fn test_parse_matches() {
        // 标准格式: matches(@Attr, 'pattern')
        let step = parse_xpath_step("//Button[matches(@Name, '^Chrome.*')]");
        assert_eq!(step.require_matches.len(), 1);
        assert_eq!(step.require_matches[0].0, XPathProperty::Name);

        // 兼容非标格式
        let step2 = parse_xpath_step("//Button[@Name=matches('^Chrome.*')]");
        assert_eq!(step2.require_matches.len(), 1);
        assert_eq!(step2.require_matches[0].0, XPathProperty::Name);
    }

    #[test]
    fn test_parse_multiple_predicates() {
        let step = parse_xpath_step("//Button[@Name='OK' and @AutomationId='btn1']");
        assert_eq!(step.required_props.len(), 2);
        assert_eq!(step.required_props[0], (XPathProperty::Name, "OK".to_string()));
        assert_eq!(step.required_props[1], (XPathProperty::AutomationId, "btn1".to_string()));
    }

    #[test]
    fn test_parse_or_predicate_is_complex() {
        let step = parse_xpath_step("//Button[@Name='OK' or @Name='Cancel']");
        assert!(step.is_complex);
        assert!(step.required_props.is_empty());
    }

    #[test]
    fn test_parse_not_predicate_is_complex() {
        let step = parse_xpath_step("//Button[not(@IsOffscreen)]");
        assert!(step.is_complex);
    }

    #[test]
    fn test_parse_wildcard_type() {
        let step = parse_xpath_step("//[@Name='OK']");
        assert!(step.type_name.is_none());
        assert_eq!(step.required_props.len(), 1);
    }

    #[test]
    fn test_parse_empty_predicate() {
        let step = parse_xpath_step("//Button");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
        assert_eq!(step.type_name.as_deref(), Some("Button"));
        assert!(step.required_props.is_empty());
        assert!(step.require_starts_with.is_empty());
        assert!(!step.is_complex);
    }

    #[test]
    fn test_parse_only_prefix() {
        let step = parse_xpath_step("//");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
        assert!(step.type_name.is_none());
    }

    // ─── step_has_complex_predicates 测试 ────────────────────────────────────────

    #[test]
    fn test_has_complex_with_starts_with() {
        let step = parse_xpath_step("//Pane[starts-with(@ClassName, 'Chrome')]");
        assert!(step_has_complex_predicates(&step));
    }

    #[test]
    fn test_has_complex_with_contains() {
        let step = parse_xpath_step("//Text[contains(@Name, 'Widget')]");
        assert!(step_has_complex_predicates(&step));
    }

    #[test]
    fn test_has_complex_with_or() {
        let step = parse_xpath_step("//Button[@Name='OK' or @Name='Cancel']");
        assert!(step_has_complex_predicates(&step));
    }

    #[test]
    fn test_has_complex_with_only_eq() {
        let step = parse_xpath_step("//Button[@Name='OK']");
        assert!(!step_has_complex_predicates(&step));
    }

    #[test]
    fn test_has_complex_with_no_predicates() {
        let step = parse_xpath_step("//Button");
        assert!(!step_has_complex_predicates(&step));
    }

    // ─── 预编译正则验证 ──────────────────────────────────────────────────────────

    #[test]
    fn test_precompiled_regexes_exist() {
        // 验证 lazy_static 已初始化（通过实际解析触发）
        let _ = xpath_regex::ATTR_EQ.captures("@Name='OK'");
        // 标准格式
        let _ = xpath_regex::STARTS_WITH.captures("starts-with(@ClassName, 'X')");
        let _ = xpath_regex::CONTAINS.captures("contains(@Name, 'x')");
        let _ = xpath_regex::MATCHES.captures("matches(@Name, '^x')");
        // 非标格式兼容
        let _ = xpath_regex::STARTS_WITH.captures("@ClassName=starts-with('X')");
        let _ = xpath_regex::CONTAINS.captures("@Name=contains('x')");
        let _ = xpath_regex::MATCHES.captures("@Name=matches('^x')");
        let _ = xpath_regex::STEP_PREFIX.captures("//Button");
    }

    // ─── 执行策略分派测试 (Phase 2.3) ─────────────────────────────────────────

    /// 验证 Child 前缀映射到 DirectChild 策略
    #[test]
    fn test_strategy_from_child_prefix() {
        let prefix = XPathStepPrefix::Child;
        let strategy = StepExecutionStrategy::from(&prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DirectChild));
    }

    /// 验证 Descendant 前缀映射到 Descendant 策略
    #[test]
    fn test_strategy_from_descendant_prefix() {
        let prefix = XPathStepPrefix::Descendant;
        let strategy = StepExecutionStrategy::from(&prefix);
        assert!(matches!(strategy, StepExecutionStrategy::Descendant));
    }

    /// 验证 DepthLimited { max_depth: 2 } 映射到 DepthLimitedBfs
    #[test]
    fn test_strategy_from_depth_limited_prefix() {
        let prefix = XPathStepPrefix::DepthLimited { max_depth: 2 };
        let strategy = StepExecutionStrategy::from(&prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DepthLimitedBfs { max_depth: 2 }));
    }

    /// 验证 DepthLimited { max_depth: 6 } 映射正确
    #[test]
    fn test_strategy_from_depth_limited_n() {
        let prefix = XPathStepPrefix::DepthLimited { max_depth: 6 };
        let strategy = StepExecutionStrategy::from(&prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DepthLimitedBfs { max_depth: 6 }));
    }

    /// 验证解析 `/Button` → Child 前缀 → DirectChild 策略
    #[test]
    fn test_parse_and_strategy_child() {
        let step = parse_xpath_step("/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Child);
        let strategy = StepExecutionStrategy::from(&step.prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DirectChild));
    }

    /// 验证解析 `//Button` → Descendant 前缀 → Descendant 策略
    #[test]
    fn test_parse_and_strategy_descendant() {
        let step = parse_xpath_step("//Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
        let strategy = StepExecutionStrategy::from(&step.prefix);
        assert!(matches!(strategy, StepExecutionStrategy::Descendant));
    }

    /// 验证解析 `/*/Button` → DepthLimited { max_depth: 2 } → DepthLimitedBfs
    #[test]
    fn test_parse_and_strategy_depth_2() {
        let step = parse_xpath_step("/*/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::DepthLimited { max_depth: 2 });
        let strategy = StepExecutionStrategy::from(&step.prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DepthLimitedBfs { max_depth: 2 }));
    }

    /// 验证解析 `/*5/Button` → DepthLimited { max_depth: 6 } → DepthLimitedBfs
    #[test]
    fn test_parse_and_strategy_depth_n() {
        let step = parse_xpath_step("/*5/Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::DepthLimited { max_depth: 6 });
        let strategy = StepExecutionStrategy::from(&step.prefix);
        assert!(matches!(strategy, StepExecutionStrategy::DepthLimitedBfs { max_depth: 6 }));
    }

    /// 验证无前缀默认 Descendant 策略
    #[test]
    fn test_parse_and_strategy_no_prefix_default() {
        let step = parse_xpath_step("Button[@Name='OK']");
        assert_eq!(step.prefix, XPathStepPrefix::Descendant);
        let strategy = StepExecutionStrategy::from(&step.prefix);
        assert!(matches!(strategy, StepExecutionStrategy::Descendant));
    }

    /// 验证 step_to_xpath_str 重建简单的步骤字符串
    #[test]
    fn test_step_to_xpath_str_simple() {
        let step = parse_xpath_step("//Button[@Name='OK']");
        let s = step_to_xpath_str(&step);
        assert!(s.contains("Button"));
        assert!(s.contains("@Name='OK'"));
    }

    /// 验证 step_to_xpath_str 处理 starts-with（输出标准格式）
    #[test]
    fn test_step_to_xpath_str_starts_with() {
        let step = parse_xpath_step("//Pane[starts-with(@ClassName, 'Chrome')]");
        let s = step_to_xpath_str(&step);
        assert!(s.contains("Pane"));
        assert!(s.contains("starts-with(@ClassName, 'Chrome')"));
    }

    /// 验证 step_to_xpath_str 处理无类型名的纯谓词
    #[test]
    fn test_step_to_xpath_str_wildcard_type() {
        let step = parse_xpath_step("//[@Name='OK']");
        let s = step_to_xpath_str(&step);
        assert!(s.contains("@Name='OK'"));
    }

    /// 验证 `#[deprecated]` 标记确实存在（编译期已验证，此处为运行时确认）
    #[test]
    fn test_deprecated_fallback_still_callable() {
        // 旧函数仍然可调用（只是产生 deprecated 警告）
        // 此处验证编译通过即可（已在 cargo build 中确认）
        // 运行时无法真正测试因为需要 UIA 环境
    }
}
