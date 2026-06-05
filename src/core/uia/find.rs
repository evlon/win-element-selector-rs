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
    find_by_xpath_control_descendants,
    find_by_xpath_detailed,
    find_by_xpath_detailed_strict,
};
use super::find_raw::{
    find_by_xpath_raw_descendants,
    find_by_xpath_raw_descendants_with_depth,
    walk_raw_tree_steps,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Main Dispatcher
// ═══════════════════════════════════════════════════════════════════════════════

pub(super) fn find_by_xpath_with_fallback(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_with_fallback_filtered(auto, window, xpath, &FindAllFilter::default())
}

pub(super) fn find_by_xpath_with_fallback_filtered(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let fallback_start = Instant::now();
    
    // ── Strip search mode suffix (:first / :onlyone / :all) ──
    let (search_mode, xpath_no_suffix) = SearchMode::strip_suffix(xpath);
    let xpath = xpath_no_suffix;
    
    // Handle parenthesized positional predicate: (xpath)[N]
    let (inner_xpath, position_index) = if xpath.starts_with('(') {
        if let Some(close) = xpath.rfind(')') {
            let after_close = &xpath[close + 1..];
            if let Some(pos) = parse_positional_predicate(after_close) {
                let inner = xpath[1..close].trim().to_string();
                log::info!("[XPath Fallback] Stripped positional wrapper: position={} inner='{}'", pos, inner);
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
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Capture Mode Prefix Detection: strip [fast]/[full] prefix from XPath.
    //
    // - [fast]: Strict ControlViewWalker only — fastest, no fallback.
    //   Used for native apps (Qt, Win32, WPF). If ControlView fails, return empty.
    // - [full]: Complete fallback chain (RawViewWalker + child HWND + cache).
    //   Used for complex apps (WebView/Chrome embedded). Can be slow but comprehensive.
    // - No prefix: Legacy behavior — full fallback chain (backward compatible).
    // ═══════════════════════════════════════════════════════════════════════════
    let (locate_mode, _, stripped_xpath) = LocateMode::strip_xpath_prefix(&inner_xpath);
    let xpath = stripped_xpath;
    let is_fast_mode = matches!(locate_mode, Some(LocateMode::Fast) | Some(LocateMode::FastChild));
    
    if is_fast_mode {
        log::info!("[XPath Fallback] [fast]/[fast-child] prefix detected — strict ControlView only, no fallback");
    } else if locate_mode.is_some() {
        log::info!("[XPath Fallback] [full]/[full-child] prefix detected — full fallback chain enabled");
    }
    
    let is_descendant = xpath.starts_with("//");
    
    // ═══════════════════════════════════════════════════════════════════════════
    // XPath Compilation Cache: Check if we already know the winning strategy.
    // ═══════════════════════════════════════════════════════════════════════════
    if let Some(cached) = cache_lookup(xpath, window) {
        let cache_start = Instant::now();
        let result = match &cached.strategy {
            CompiledStrategy::WindowFastPath => {
                log::info!("[XPath Cache] WindowFastPath cached — should be handled at caller level, falling through");
                None
            }
            CompiledStrategy::ControlViewDirect => {
                log::info!("[XPath Cache] Executing cached: ControlViewDirect");
                find_by_xpath_detailed(auto, window, xpath, None).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::RawViewBfs => {
                log::info!("[XPath Cache] Executing cached: RawViewBfs");
                cached_raw_view_bfs(auto, window, xpath, filter).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::ContentRoot => {
                log::info!("[XPath Cache] Executing cached: ContentRoot");
                if let Some(cr) = find_content_root(auto, window) {
                    find_by_xpath_detailed(auto, &cr, xpath, None).ok()
                        .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
                } else { None }
            }
            CompiledStrategy::FindAllDescendants => {
                log::info!("[XPath Cache] Executing cached: FindAllDescendants");
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                find_by_xpath_raw_descendants(auto, window, &desc_xpath, SearchMode::First, filter).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::ChildHwndEnum(child_idx) => {
                log::info!("[XPath Cache] Executing cached: ChildHwndEnum({})", child_idx);
                cached_child_hwnd_search(auto, window, xpath, *child_idx, filter).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::SiblingWindow => {
                log::info!("[XPath Cache] Executing cached: SiblingWindow");
                cached_sibling_search(auto, window, xpath).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::ChildProcessWindow => {
                log::info!("[XPath Cache] Executing cached: ChildProcessWindow");
                cached_child_process_search(auto, window, xpath).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::DescendantContentRoot => {
                log::info!("[XPath Cache] Executing cached: DescendantContentRoot");
                if let Some(cr) = find_content_root(auto, window) {
                    find_by_xpath_detailed(auto, &cr, xpath, None).ok()
                        .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
                } else { None }
            }
            CompiledStrategy::DescendantWindowRoot => {
                log::info!("[XPath Cache] Executing cached: DescendantWindowRoot");
                find_by_xpath_detailed(auto, window, xpath, None).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::DescendantRawWalk => {
                log::info!("[XPath Cache] Executing cached: DescendantRawWalk");
                find_by_xpath_raw_descendants(auto, window, xpath, SearchMode::First, filter).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::DescendantChildHwnd(child_idx) => {
                log::info!("[XPath Cache] Executing cached: DescendantChildHwnd({})", child_idx);
                cached_descendant_child_hwnd(auto, window, xpath, *child_idx, filter).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
        };
        
        if let Some((results, segments)) = result {
            let cache_elapsed = cache_start.elapsed().as_millis() as u64;
            log::info!(
                "[XPath Cache] ✓ Cached strategy succeeded in {}ms (avg={}ms, hits={})",
                cache_elapsed, cached.avg_time_ms, cached.hit_count
            );
            cache_store(xpath, window, cached.strategy.clone(), cache_elapsed);
            let mut results = results;
            if let Some(pos) = position_index {
                if pos > 0 && results.len() >= pos {
                    results = vec![results.swap_remove(pos - 1)];
                } else if !results.is_empty() {
                    results.clear();
                }
            }
            results = apply_search_mode_ui(results, search_mode);
            return Ok((results, segments));
        }
        
        log::info!("[XPath Cache] Cached strategy failed (stale?), falling back to full chain");
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Helper: Record successful strategy in cache and return results.
    // ═══════════════════════════════════════════════════════════════════════════
    #[inline(always)]
    fn record_and_return(
        strategy: CompiledStrategy,
        results: Vec<UIElement>,
        segments: Vec<SegmentValidationResult>,
        xpath: &str,
        window: &UIElement,
        position_index: Option<usize>,
        fallback_start: &std::time::Instant,
        search_mode: SearchMode,
    ) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
        let elapsed = fallback_start.elapsed().as_millis() as u64;
        cache_store(xpath, window, strategy, elapsed);
        let mut results = results;
        if let Some(pos) = position_index {
            if pos > 0 && results.len() >= pos {
                log::info!("[XPath Fallback] Positional predicate [{}]: selecting match {} of {}", 
                    pos, pos, results.len());
                results = vec![results.swap_remove(pos - 1)];
            } else if !results.is_empty() {
                log::info!("[XPath Fallback] Positional predicate [{}]: only {} results, position out of range", 
                    pos, results.len());
                results.clear();
            }
        }
        results = apply_search_mode_ui(results, search_mode);
        Ok((results, segments))
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Fast Mode ([fast] / [fast-child] prefix): Strict ControlViewWalker only.
    // → All ControlView logic lives in find_control.rs
    // ═══════════════════════════════════════════════════════════════════════════
    if is_fast_mode {
        log::info!("[PERF][FAST] Fast mode enter: descendant={} at {}ms", is_descendant, fallback_start.elapsed().as_millis());
        
        let t_fast = std::time::Instant::now();
        if is_descendant {
            if let Ok((r, s)) = find_by_xpath_control_descendants(auto, window, xpath, search_mode, filter) {
                log::info!("[PERF][FAST] control_descendants done: {}ms, {} results", t_fast.elapsed().as_millis(), r.len());
                if !r.is_empty() {
                    let elapsed = fallback_start.elapsed().as_millis() as u64;
                    log::info!("[XPath Fallback] ✓ [fast] Found {} results via ControlView FindAll ({}ms)", r.len(), elapsed);
                    let mut results = r;
                    if let Some(pos) = position_index {
                        if pos > 0 && results.len() >= pos {
                            results = vec![results.swap_remove(pos - 1)];
                        } else if !results.is_empty() {
                            results.clear();
                        }
                    }
                    results = apply_search_mode_ui(results, search_mode);
                    return Ok((results, s));
                }
            }
            log::info!("[XPath Fallback] ✗ [fast] ControlView FindAll not found — returning empty");
            return Ok((vec![], vec![]));
        } else {
            if let Ok((r, s)) = find_by_xpath_detailed_strict(auto, window, xpath) {
                log::info!("[PERF][FAST] select_nodes_strict done: {}ms, {} results", t_fast.elapsed().as_millis(), r.len());
                if !r.is_empty() {
                    let elapsed = fallback_start.elapsed().as_millis() as u64;
                    log::info!("[XPath Fallback] ✓ [fast] Found {} results via ControlView ({}ms)", r.len(), elapsed);
                    let mut results = r;
                    if let Some(pos) = position_index {
                        if pos > 0 && results.len() >= pos {
                            results = vec![results.swap_remove(pos - 1)];
                        } else if !results.is_empty() {
                            results.clear();
                        }
                    }
                    results = apply_search_mode_ui(results, search_mode);
                    return Ok((results, s));
                }
            }
            log::info!("[XPath Fallback] ✗ [fast] ControlView not found — strict mode, returning empty");
            return Ok((vec![], vec![]));
        }
    }
    
    if is_descendant {
        // ═══════════════════════════════════════════════════════════════════════════
        // Descendant XPath (//...) — [full]/[full-child] strategy
        //
        // CORE PRINCIPLE: [full] uses RawView ONLY, never ControlView!
        //
        // - [fast]/[fast-child] → ControlView only (handled above, already returned)
        // - [full]/[full-child] → RawView only (here)
        // - No prefix → RawView only (legacy = full, backward compatible)
        //
        // → All RawView logic lives in find_raw.rs
        // ═══════════════════════════════════════════════════════════════════════════
        
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        // Step 1: RawView search from window root
        log::info!("[XPath Fallback] //XPath — Step 1: RawView descendants from window root");
        if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, window, xpath, SearchMode::First, filter) {
            if !r.is_empty() {
                log::info!("[XPath Fallback] ✓ Step 1: Found {} results via RawView descendants ({}ms)", 
                    r.len(), fallback_start.elapsed().as_millis());
                return record_and_return(CompiledStrategy::DescendantRawWalk, r, s, xpath, window, position_index, &fallback_start, search_mode);
            }
        }
        
        // Step 2: EnumChildWindows — try child HWNDs (RawView)
        if let Ok(handle) = window.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
            log::info!("[XPath Fallback] //XPath — Step 2: trying {} child HWNDs (RawView)", child_hwnds.len());

            // Categorize child HWNDs: non-WebView first, WebView second
            let mut non_webview_children: Vec<(usize, UIElement)> = Vec::new();
            let mut webview_children: Vec<(usize, UIElement)> = Vec::new();

            for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    let child_class = child_elem.get_classname().unwrap_or_default();
                    let is_webview = is_webview_class(&child_class);
                    log::info!("[XPath Fallback]   child HWND[{}]: class='{}' webview={}",
                        idx, child_class, is_webview);
                    if is_webview {
                        webview_children.push((idx, child_elem));
                    } else {
                        non_webview_children.push((idx, child_elem));
                    }
                }
            }

            // Phase 2a: Search non-WebView children first (faster, smaller subtrees)
            for (idx, child_elem) in &non_webview_children {
                if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, child_elem, xpath, SearchMode::First, filter) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Step 2a: Found {} from non-WebView child HWND[{}] via RawView ({}ms)",
                            r.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::DescendantChildHwnd(*idx), r, s, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }

            // Phase 2b: Search WebView children (slow, large subtrees)
            for (idx, child_elem) in &webview_children {
                log::info!("[XPath Fallback]   Phase 2b: trying WebView child HWND[{}] via RawView ({}ms elapsed)",
                    idx, fallback_start.elapsed().as_millis());
                if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, child_elem, xpath, SearchMode::First, filter) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Step 2b: Found {} from WebView child HWND[{}] via RawView ({}ms)",
                            r.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::DescendantChildHwnd(*idx), r, s, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }
        }
        
        log::info!("[XPath Fallback] All //XPath fallbacks exhausted ({}ms)", 
            fallback_start.elapsed().as_millis());
        Ok((vec![], vec![]))
    } else {
        // ── Absolute XPath (/...): optimized multi-strategy approach ──

        let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        let first_step_parsed = if !xpath_parts.is_empty() {
            Some(parse_xpath_step(xpath_parts[0]))
        } else {
            None
        };
        
        let first_step_is_webview = first_step_parsed.as_ref().map_or(false, |parsed| {
            let has_webview_class = parsed.required_props.iter().any(|(k, v)| {
                k == "ClassName" && is_webview_class(v)
            }) || parsed.require_starts_with.iter().any(|(k, v)| {
                k == "ClassName" && is_webview_class(v)
            });
            let has_chrome_fwid = parsed.required_props.iter().any(|(k, v)| {
                k == "FrameworkId" && (v.eq_ignore_ascii_case("Chrome") || v.eq_ignore_ascii_case("WebView"))
            });
            has_webview_class || has_chrome_fwid
        });
        
        // Strategy 1: Try from window root (RawViewWalker/uiauto-xpath)
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 1: SKIPPED (first step has WebView class, ControlViewWalker won't find it)");
        } else {
            log::info!("[XPath Fallback] /XPath — Strategy 1: window root (primary)");
            let (results, segments) = find_by_xpath_detailed(auto, window, xpath, None)?;
            if !results.is_empty() {
                log::info!("[XPath Fallback] ✓ Strategy 1: Found {} from window root ({}ms)", 
                    results.len(), fallback_start.elapsed().as_millis());
                return record_and_return(CompiledStrategy::ControlViewDirect, results, segments, xpath, window, position_index, &fallback_start, search_mode);
            }
        }
        
        // Strategy 1.5: RawViewWalker BFS from window root.
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 1.5: SKIPPED (WebView elements are under child HWNDs, not window root)");
        } else if let Some(ref first_parsed) = first_step_parsed {
            if !xpath_parts.is_empty() {
                log::info!("[XPath Fallback] /XPath — Strategy 1.5: FindAll from window root");
                let first_step_end = find_first_step_end(xpath);
                let remaining_after_first = &xpath[first_step_end..];

                let condition = build_uia_condition_from_step(auto, first_parsed);
                let has_complex = step_has_complex_predicates(first_parsed);

                let candidates: Vec<UIElement> = if let Some(cond) = condition {
                    match window.find_all(TreeScope::Subtree, &cond) {
                        Ok(raw_candidates) => {
                            log::info!("[XPath Fallback] Strategy 1.5: FindAll returned {} candidates", raw_candidates.len());
                            let filtered = if has_complex {
                                raw_candidates
                                    .into_iter()
                                    .filter(|c| element_matches_parsed_step(c, first_parsed))
                                    .collect()
                            } else {
                                raw_candidates
                            };
                            filter_findall_results(window, filtered, "Strat1.5", filter)
                        }
                        Err(_) => Vec::new(),
                    }
                } else {
                    Vec::new()
                };

                for c in &candidates {
                    log::info!("[XPath Fallback] Strategy 1.5: trying remaining XPath from candidate");
                    if let Some(result) = try_remaining_from_match(
                        auto, c, remaining_after_first, &xpath_parts,
                        &fallback_start, "1.5", filter,
                    ) {
                        let (r, s) = result;
                        return record_and_return(CompiledStrategy::RawViewBfs, r, s, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
                log::info!("[XPath Fallback] Strategy 1.5: no match found via FindAll");
            }
        }
        
        // Strategy 2: Try content root if available
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 2: SKIPPED (looking for WebView container, content root is below it)");
        } else {
            log::info!("[XPath Fallback] /XPath — Strategy 2: trying content root...");
            if let Some(content_root) = find_content_root(auto, window) {
                if let Ok((r2, s2)) = find_by_xpath_detailed(auto, &content_root, xpath, None) {
                    if !r2.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2a: Found {} from content root ({}ms)", 
                            r2.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ContentRoot, r2, s2, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
                
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                log::info!("[XPath Fallback] /XPath — Strategy 2b: content root descendant");
                if let Ok((r3, s3)) = find_by_xpath_detailed(auto, &content_root, &desc_xpath, None) {
                    if !r3.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2b: Found {} from content root desc ({}ms)", 
                            r3.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ContentRoot, r3, s3, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }
        }
        
        // Strategy 2.5: FindAll(Descendants) raw tree search
        {
            if first_step_is_webview {
                log::info!("[XPath Fallback] /XPath — Skipping Strategy 2.5: first step has WebView class, going directly to 2.7");
            } else {
                log::info!("[XPath Fallback] /XPath — Strategy 2.5: FindAll(Descendants) raw tree search");
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r25, s25)) = find_by_xpath_raw_descendants(auto, window, &desc_xpath, SearchMode::First, filter) {
                    if !r25.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.5: Found {} via raw descendant search ({}ms)", 
                            r25.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::FindAllDescendants, r25, s25, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }
        }
        
        // Strategy 2.7: Search child HWNDs via EnumChildWindows
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted before Strategy 2.7 ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        log::info!("[XPath Fallback] /XPath — Strategy 2.7: child HWND search via EnumChildWindows");
        
        if let Ok(handle) = window.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));

            // Categorize child HWNDs
            let mut non_webview_children: Vec<(usize, UIElement)> = Vec::new();
            let mut webview_children: Vec<(usize, UIElement)> = Vec::new();

            for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    let child_class = child_elem.get_classname().unwrap_or_default();
                    if is_webview_class(&child_class) {
                        webview_children.push((idx, child_elem));
                    } else {
                        non_webview_children.push((idx, child_elem));
                    }
                }
            }

            // Phase 2.7a: non-WebView child HWNDs
            for (idx, child_elem) in &non_webview_children {
                log::info!("[XPath Fallback] /XPath — Strategy 2.7a: non-WebView child HWND[{}]", idx);
                if let Ok((r, s)) = find_by_xpath_detailed(auto, child_elem, xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.7a: Found {} from non-WebView child[{}] ({}ms)",
                            r.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildHwndEnum(*idx), r, s, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r2, s2)) = find_by_xpath_detailed(auto, child_elem, &desc_xpath, None) {
                    if !r2.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.7a desc: Found {} from non-WebView child[{}] ({}ms)",
                            r2.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildHwndEnum(*idx), r2, s2, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }

            // Phase 2.7b: WebView child HWNDs
            for (idx, child_elem) in &webview_children {
                log::info!("[XPath Fallback] /XPath — Strategy 2.7b: WebView child HWND[{}] ({}ms elapsed)",
                    idx, fallback_start.elapsed().as_millis());
                if let Ok((r, s)) = find_by_xpath_detailed(auto, child_elem, xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.7b: Found {} from WebView child[{}] ({}ms)",
                            r.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildHwndEnum(*idx), r, s, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r2, s2)) = find_by_xpath_detailed(auto, child_elem, &desc_xpath, None) {
                    if !r2.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.7b desc: Found {} from WebView child[{}] ({}ms)",
                            r2.len(), idx, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildHwndEnum(*idx), r2, s2, xpath, window, position_index, &fallback_start, search_mode);
                    }
                }
            }
        }

        // Strategy 3: Try sibling windows (same process)
        log::info!("[XPath Fallback] /XPath — Strategy 3: sibling window search");
        if let Ok((r, s)) = cached_sibling_search(auto, window, xpath) {
            if !r.is_empty() {
                log::info!("[XPath Fallback] ✓ Strategy 3: Found {} from sibling window ({}ms)",
                    r.len(), fallback_start.elapsed().as_millis());
                return record_and_return(CompiledStrategy::SiblingWindow, r, s, xpath, window, position_index, &fallback_start, search_mode);
            }
        }

        // Strategy 4: Try child process windows
        log::info!("[XPath Fallback] /XPath — Strategy 4: child process window search");
        if let Ok((r, s)) = cached_child_process_search(auto, window, xpath) {
            if !r.is_empty() {
                log::info!("[XPath Fallback] ✓ Strategy 4: Found {} from child process window ({}ms)",
                    r.len(), fallback_start.elapsed().as_millis());
                return record_and_return(CompiledStrategy::ChildProcessWindow, r, s, xpath, window, position_index, &fallback_start, search_mode);
            }
        }
        
        log::info!("[XPath Fallback] All /XPath fallbacks exhausted ({}ms)", 
            fallback_start.elapsed().as_millis());
        Ok((vec![], vec![]))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cache Strategy Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn cached_raw_view_bfs(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_raw_descendants(auto, window, xpath, SearchMode::First, filter)
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
        if let Ok((r, s)) = find_by_xpath_detailed(auto, &child_elem, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    find_by_xpath_raw_descendants(auto, &child_elem, xpath, SearchMode::First, filter)
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
        if let Ok((r, s)) = find_by_xpath_detailed(auto, sibling, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
        let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
        if let Ok((r, s)) = find_by_xpath_detailed(auto, sibling, &desc_xpath, None) {
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
        if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
        if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, &desc_xpath, None) {
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
        if let Ok((r, s)) = find_by_xpath_detailed(auto, &child_elem, xpath, None) {
            if !r.is_empty() { return Ok((r, s)); }
        }
    }
    find_by_xpath_raw_descendants(auto, &child_elem, xpath, SearchMode::First, filter)
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
        let prop: UIProperty = match key.as_str() {
            "Name" => UIProperty::Name,
            "AutomationId" => UIProperty::AutomationId,
            "FrameworkId" => UIProperty::FrameworkId,
            "ClassName" => UIProperty::ClassName,
            "ControlType" => continue,
            _ => continue,
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

pub(super) fn step_has_complex_predicates(step: &ParsedXPathStep) -> bool {
    !step.require_starts_with.is_empty()
        || !step.require_contains.is_empty()
        || !step.require_matches.is_empty()
}

pub(super) fn parse_xpath_step(step: &str) -> ParsedXPathStep {
    let (type_name, predicates_str): (Option<String>, &str) = if step.starts_with('[') {
        (None, step)
    } else if let Some(bracket_pos) = step.find('[') {
        (Some(step[..bracket_pos].to_string()), &step[bracket_pos..])
    } else {
        (Some(step.to_string()), "")
    };

    // Detect `or` or `not()` — cannot be reliably handled by simple key=value
    if predicates_str.contains(" or ") || predicates_str.contains("not(") {
        return ParsedXPathStep {
            type_name,
            required_props: Default::default(),
            require_starts_with: Default::default(),
            require_contains: Default::default(),
            require_matches: Default::default(),
        };
    }

    let mut required_props: Vec<(String, String)> = Vec::new();
    let mut require_starts_with: Vec<(String, String)> = Vec::new();
    let mut require_contains: Vec<(String, String)> = Vec::new();
    let mut require_matches: Vec<(String, Regex)> = Vec::new();

    // Parse [@Attr='Value'] style predicates
    let attr_re = Regex::new(r#"@(\w+)\s*=\s*'([^']*)'"#).unwrap();
    for cap in attr_re.captures_iter(predicates_str) {
        let key = cap[1].to_string();
        let value = cap[2].to_string();
        required_props.push((key, value));
    }

    // Parse [@Attr=starts-with('Value')]
    let starts_re = Regex::new(r#"@(\w+)\s*=\s*starts-with\(\s*'([^']*)'\s*\)"#).unwrap();
    for cap in starts_re.captures_iter(predicates_str) {
        let key = cap[1].to_string();
        let value = cap[2].to_string();
        require_starts_with.push((key, value));
    }

    // Parse [@Attr=contains('Value')]
    let contains_re = Regex::new(r#"@(\w+)\s*=\s*contains\(\s*'([^']*)'\s*\)"#).unwrap();
    for cap in contains_re.captures_iter(predicates_str) {
        let key = cap[1].to_string();
        let value = cap[2].to_string();
        require_contains.push((key, value));
    }

    // Parse [@Attr=matches('Value')] or [@Attr=matches('Value','flags')]
    let matches_re = Regex::new(r#"@(\w+)\s*=\s*matches\(\s*'([^']*)'\s*(?:,\s*'([^']*)'\s*)?\)"#).unwrap();
    for cap in matches_re.captures_iter(predicates_str) {
        let key = cap[1].to_string();
        let pattern = cap[2].to_string();
        let flags = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let full_pattern = if flags.is_empty() {
            format!("(?i){}", pattern)
        } else {
            format!("(?{}){}", flags, pattern)
        };
        if let Ok(re) = Regex::new(&full_pattern) {
            require_matches.push((key, re));
        }
    }

    ParsedXPathStep {
        type_name,
        required_props,
        require_starts_with,
        require_contains,
        require_matches,
    }
}

fn get_uia_property_for_xpath(elem: &UIElement, key: &str) -> String {
    match key {
        "Name" => elem.get_name().unwrap_or_default(),
        "AutomationId" => elem.get_automation_id().unwrap_or_default(),
        "ClassName" => elem.get_classname().unwrap_or_default(),
        "FrameworkId" => elem.get_framework_id().unwrap_or_default(),
        "ControlType" => elem.get_control_type().map(|ct| format!("{:?}", ct)).unwrap_or_default(),
        _ => String::new(),
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

    true
}

/// Find the byte offset where the first XPath step ends in the original string.
fn find_first_step_end(xpath: &str) -> usize {
    let bytes = xpath.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'/' {
        i += 1;
    }
    let mut bracket_depth: i32 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                }
            }
            b'/' => {
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Helper: given an element that matches the first XPath step, try to resolve
/// the remaining steps via uiauto-xpath then raw tree walk.
fn try_remaining_from_match(
    auto: &UIAutomation,
    match_elem: &UIElement,
    remaining_xpath: &str,
    xpath_parts: &[&str],
    fallback_start: &std::time::Instant,
    strategy_label: &str,
    filter: &FindAllFilter,
) -> Option<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let remaining_has_descendant = remaining_xpath.contains("//");

    // Fast path for // descendant searches: use FindAll instead of uiauto-xpath
    if remaining_has_descendant {
        if let Some(pos) = remaining_xpath.rfind("//") {
            let final_step_str = &remaining_xpath[pos + 2..];
            let final_step_parsed = parse_xpath_step(final_step_str);
            let has_complex = step_has_complex_predicates(&final_step_parsed);
            let condition = build_uia_condition_from_step(auto, &final_step_parsed);

            if let Some(cond) = condition {
                if !has_complex || !final_step_parsed.require_starts_with.is_empty()
                    || !final_step_parsed.require_contains.is_empty()
                    || !final_step_parsed.require_matches.is_empty()
                {
                    match match_elem.find_all(TreeScope::Subtree, &cond) {
                        Ok(candidates) => {
                            let filtered: Vec<UIElement> = if has_complex {
                                candidates.into_iter()
                                    .filter(|c| element_matches_parsed_step(c, &final_step_parsed))
                                    .collect()
                            } else {
                                candidates
                            };
                            let results = filter_findall_results(match_elem, filtered, "FastPath", filter);
                            if !results.is_empty() {
                                log::info!("[XPath Fallback] Strategy {}: FindAll fast path found {} ({}ms)",
                                    strategy_label, results.len(), fallback_start.elapsed().as_millis());
                                let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                                    SegmentValidationResult {
                                        segment_index: i,
                                        segment_text: step.to_string(),
                                        matched: i < xpath_parts.len() - 1 || !results.is_empty(),
                                        match_count: if i == xpath_parts.len() - 1 { results.len() } else { 0 },
                                        duration_ms: 0,
                                        predicate_failures: Vec::new(),
                                    }
                                }).collect();
                                return Some((results, segments));
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
        }
    }

    // Standard path: uiauto-xpath for remaining steps (skip if contains //)
    if !remaining_has_descendant {
        if let Ok((matches, segments)) = find_by_xpath_detailed(auto, match_elem, remaining_xpath, None) {
            if !matches.is_empty() {
                log::info!("[XPath Fallback] Strategy {}: uiauto-xpath found {} ({}ms)",
                    strategy_label, matches.len(), fallback_start.elapsed().as_millis());
                return Some((matches, segments));
            }
        }
    }

    // Raw tree walk fallback
    if let Ok(raw_walker) = auto.get_raw_view_walker() {
        let remaining_parts: Vec<&str> = remaining_xpath.split('/').filter(|s| !s.is_empty()).collect();
        if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, match_elem, &remaining_parts) {
            if !matches.is_empty() {
                log::info!("[XPath Fallback] Strategy {}: raw walk found {} ({}ms)",
                    strategy_label, matches.len(), fallback_start.elapsed().as_millis());
                let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                    SegmentValidationResult {
                        segment_index: i,
                        segment_text: step.to_string(),
                        matched: i < xpath_parts.len() - 1 || !matches.is_empty(),
                        match_count: if i == xpath_parts.len() - 1 { matches.len() } else { 0 },
                        duration_ms: 0,
                        predicate_failures: Vec::new(),
                    }
                }).collect();
                return Some((matches, segments));
            }
        }
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

pub fn find_all_elements_detailed(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
    search_context: Option<&crate::core::model::SearchContext>,
    _timeout_ms: Option<u64>,
    filter: Option<&FindAllFilter>,
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
                let (elements, _) = match find_by_xpath_with_fallback_filtered(&auto, &child_elem, &child_xpath, &filter) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if !elements.is_empty() {
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

        let (elements, _) = match find_by_xpath_with_fallback_filtered(&auto, window, element_xpath_no_suffix, &filter) {
            Ok(result) => result,
            Err(_) => continue,
        };
        
        if !elements.is_empty() {
            let mut rng = rand::thread_rng();
            let results: Vec<_> = elements.iter().filter_map(|elem| {
                element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
            }).collect();
            return apply_search_mode(results, search_mode);
        }
    }
    
    apply_search_mode(vec![], search_mode)
}

/// Apply SearchMode (:first / :onlyone / :all) post-processing to UIElement results.
#[inline]
fn apply_search_mode_ui(
    results: Vec<UIElement>,
    mode: SearchMode,
) -> Vec<UIElement> {
    match mode {
        SearchMode::All => results,
        SearchMode::First => {
            if results.len() > 1 {
                log::info!("[SearchMode:first] Truncating {} UIElements to 1", results.len());
                results.into_iter().take(1).collect()
            } else {
                results
            }
        }
        SearchMode::OnlyOne => {
            if results.len() > 1 {
                log::warn!("[SearchMode:onlyone] Expected unique element, found {} UIElements — returning empty", results.len());
                vec![]
            } else {
                results
            }
        }
    }
}

/// Apply SearchMode (:first / :onlyone / :all) post-processing to ElementData results.
#[inline]
fn apply_search_mode(
    results: Vec<crate::core::model::ElementData>,
    mode: SearchMode,
) -> Vec<crate::core::model::ElementData> {
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
                log::warn!("[SearchMode:onlyone] Expected unique element, found {} results — returning empty", results.len());
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
    let (elements, _) = match find_by_xpath_detailed(&auto, &desktop, element_xpath, None) {
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

fn find_from_element_convert(
    _auto: &UIAutomation,
    base_elem: &UIElement,
    raw_elements: &[UIElement],
    random_range: f32,
) -> Vec<crate::core::model::ElementData> {
    let base_rect = base_elem.get_bounding_rectangle().ok().map(|r| {
        crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });
    let mut rng = rand::thread_rng();
    raw_elements.iter().filter_map(|elem| {
        element_info_from_uia(elem, base_rect.as_ref(), random_range, &mut rng)
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

    let (raw_elements, _) = match find_by_xpath_detailed(auto, base_elem, xpath, None) {
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

pub fn find_from_element_cached(
    runtime_id: &str,
    xpath: &str,
    random_range: f32,
    search_strategy: Option<crate::core::model::SearchStrategy>,
) -> Vec<crate::core::model::ElementData> {
    find_from_element_cached_filtered(runtime_id, xpath, random_range, search_strategy, &FindAllFilter::default())
}

pub fn find_from_element_cached_filtered(
    runtime_id: &str,
    xpath: &str,
    random_range: f32,
    search_strategy: Option<crate::core::model::SearchStrategy>,
    filter: &FindAllFilter,
) -> Vec<crate::core::model::ElementData> {
    use crate::core::element_cache::{cache_element, get_cached_element};

    let (search_mode, xpath) = SearchMode::strip_suffix(xpath);

    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };

    let base_elem: UIElement = match get_cached_element(runtime_id) {
        Some(e) => e,
        None => {
            log::warn!("[find_from_element] Element not found in cache: runtime_id={}", runtime_id);
            return vec![];
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

    let raw_elements: Vec<UIElement> = match search_strategy {
        Some(crate::core::model::SearchStrategy::Fast { max_depth }) => {
            log::info!("[find_from_element] Fast strategy, max_depth={}", max_depth);
            match find_by_xpath_detailed(&auto, &base_elem, xpath, None) {
                Ok((elems, _)) => elems,
                Err(e) => {
                    log::warn!("[find_from_element] Fast strategy failed: {}", e);
                    vec![]
                }
            }
        }
        Some(crate::core::model::SearchStrategy::Full { max_depth }) => {
            log::info!("[find_from_element] Full strategy, max_depth={}", max_depth);
            match find_by_xpath_raw_descendants_with_depth(&auto, &base_elem, xpath, max_depth, search_mode, filter) {
                Ok((elems, _)) => elems,
                Err(e) => {
                    log::warn!("[find_from_element] Full strategy failed: {}", e);
                    vec![]
                }
            }
        }
        _ => {
            // Adaptive or None → try Fast first, then fall back to Full
            log::info!("[find_from_element] Adaptive strategy: trying Fast first");
            let (results, raw) = match find_by_xpath_detailed(&auto, &base_elem, xpath, None) {
                Ok((elems, _segs)) if !elems.is_empty() => {
                    log::info!("[find_from_element] Adaptive: Fast succeeded ({} elements)", elems.len());
                    (find_from_element_convert(&auto, &base_elem, &elems, random_range), elems)
                }
                Ok((_, _)) => {
                    log::info!("[find_from_element] Adaptive: Fast found nothing, trying Full (RawView BFS)");
                    match find_by_xpath_raw_descendants_with_depth(&auto, &base_elem, xpath, 8, search_mode, filter) {
                        Ok((elems, _)) if !elems.is_empty() => {
                            log::info!("[find_from_element] Adaptive: Full succeeded ({} elements)", elems.len());
                            (find_from_element_convert(&auto, &base_elem, &elems, random_range), elems)
                        }
                        _ => {
                            log::info!("[find_from_element] Adaptive: both Fast and Full found nothing");
                            return vec![];
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[find_from_element] Adaptive: Fast errored ({}), trying Full", e);
                    match find_by_xpath_raw_descendants_with_depth(&auto, &base_elem, xpath, 8, search_mode, filter) {
                        Ok((elems, _)) if !elems.is_empty() => {
                            log::info!("[find_from_element] Adaptive: Full succeeded after Fast error ({} elements)", elems.len());
                            (find_from_element_convert(&auto, &base_elem, &elems, random_range), elems)
                        }
                        _ => {
                            log::info!("[find_from_element] Adaptive: both Fast and Full found nothing after error");
                            return vec![];
                        }
                    }
                }
            };

            for raw_elem in &raw {
                if let Some(rid_str) = runtime_id_key(raw_elem).map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")) {
                    cache_element(rid_str, raw_elem.clone());
                }
            }
            return apply_search_mode(results, search_mode);
        }
    };

    // Cache found elements
    for raw_elem in &raw_elements {
        if let Some(rid_str) = runtime_id_key(raw_elem).map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")) {
            cache_element(rid_str, raw_elem.clone());
        }
    }

    let mut rng = rand::thread_rng();
    let results: Vec<_> = raw_elements.iter().filter_map(|elem| {
        element_info_from_uia(elem, base_rect.as_ref(), random_range, &mut rng)
    }).collect();
    apply_search_mode(results, search_mode)
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

    let filtered_count = filtered.len();
    if filtered_count < raw_count {
        log::info!("[FindAll Filter][{}] Post-filter: {} → {} elements (removed {}: offscreen/zero-size/out-of-bounds)",
            label, raw_count, filtered_count, raw_count - filtered_count);
    }

    filtered
}
