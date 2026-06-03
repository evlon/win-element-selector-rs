use super::*;

pub(super) fn find_by_xpath_with_fallback(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let fallback_start = Instant::now();
    
    // Handle parenthesized positional predicate: (xpath)[N]
    // Strip the wrapper, search with the inner path, then select the Nth result.
    // E.g., (/Pane[...]/Group[...])[1] → search /Pane[...]/Group[...] → return 1st match
    let (inner_xpath, position_index) = if xpath.starts_with('(') {
        if let Some(close) = xpath.rfind(')') {
            let after_close = &xpath[close + 1..];
            if let Some(pos) = parse_positional_predicate(after_close) {
                let inner = xpath[1..close].trim().to_string();
                log::info!("[XPath Fallback] Stripped positional wrapper: position={} inner='{}'", pos, inner);
                (inner, Some(pos))
            } else {
                // Parentheses but no [N] after — just strip the parens
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
    let (capture_mode, stripped_xpath) = CaptureMode::strip_xpath_prefix(&inner_xpath);
    let xpath = stripped_xpath;
    let is_fast_mode = matches!(capture_mode, Some(CaptureMode::Fast) | Some(CaptureMode::FastChild));
    
    if is_fast_mode {
        log::info!("[XPath Fallback] [fast]/[fast-child] prefix detected — strict ControlView only, no fallback");
    } else if capture_mode.is_some() {
        log::info!("[XPath Fallback] [full]/[full-child] prefix detected — full fallback chain enabled");
    }
    
    let is_descendant = xpath.starts_with("//");
    
    // ═══════════════════════════════════════════════════════════════════════════
    // XPath Compilation Cache: Check if we already know the winning strategy.
    //
    // First execution: cache miss → run full fallback chain → record winner.
    // Subsequent executions: cache hit → jump directly to winning strategy.
    // This eliminates the 200-3000ms fallback trial-and-error on repeated calls.
    // ═══════════════════════════════════════════════════════════════════════════
    if let Some(cached) = cache_lookup(xpath, window) {
        let cache_start = Instant::now();
        let result = match &cached.strategy {
            CompiledStrategy::WindowFastPath => {
                // Window fast path is handled by find_all_elements_detailed caller,
                // not here. Fall through to normal execution.
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
                cached_raw_view_bfs(auto, window, xpath).ok()
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
                find_by_xpath_raw_descendants(auto, window, &desc_xpath).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::ChildHwndEnum(child_idx) => {
                log::info!("[XPath Cache] Executing cached: ChildHwndEnum({})", child_idx);
                cached_child_hwnd_search(auto, window, xpath, *child_idx).ok()
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
                find_by_xpath_raw_descendants(auto, window, xpath).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
            CompiledStrategy::DescendantChildHwnd(child_idx) => {
                log::info!("[XPath Cache] Executing cached: DescendantChildHwnd({})", child_idx);
                cached_descendant_child_hwnd(auto, window, xpath, *child_idx).ok()
                    .and_then(|(r, s)| if r.is_empty() { None } else { Some((r, s)) })
            }
        };
        
        if let Some((results, segments)) = result {
            let cache_elapsed = cache_start.elapsed().as_millis() as u64;
            log::info!(
                "[XPath Cache] ✓ Cached strategy succeeded in {}ms (avg={}ms, hits={})",
                cache_elapsed, cached.avg_time_ms, cached.hit_count
            );
            // Update avg time with new sample
            cache_store(xpath, window, cached.strategy.clone(), cache_elapsed);
            // Apply positional predicate
            let mut results = results;
            if let Some(pos) = position_index {
                if pos > 0 && results.len() >= pos {
                    results = vec![results.swap_remove(pos - 1)];
                } else if !results.is_empty() {
                    results.clear();
                }
            }
            return Ok((results, segments));
        }
        
        // Cached strategy failed (window state changed, element no longer exists, etc.)
        // Fall through to full fallback chain and re-learn.
        log::info!("[XPath Cache] Cached strategy failed (stale?), falling back to full chain");
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Helper: Record successful strategy in cache and return results.
    // Called at every successful return point in the fallback chain.
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
    ) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
        let elapsed = fallback_start.elapsed().as_millis() as u64;
        cache_store(xpath, window, strategy, elapsed);
        // Apply positional predicate
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
        Ok((results, segments))
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Fast Mode ([fast] prefix): Strict ControlViewWalker only.
    // No fallback, no child HWND, no cache warming — just direct find_by_xpath_detailed.
    // Returns empty immediately if ControlView doesn't find the element.
    // ═══════════════════════════════════════════════════════════════════════════
    if is_fast_mode {
        log::info!("[XPath Fallback] [fast] — strict ControlView only");
        if let Ok((r, s)) = find_by_xpath_detailed_strict(auto, window, xpath) {
            if !r.is_empty() {
                let elapsed = fallback_start.elapsed().as_millis() as u64;
                log::info!("[XPath Fallback] ✓ [fast] Found {} results via ControlView ({}ms)", r.len(), elapsed);
                // Apply positional predicate
                let mut results = r;
                if let Some(pos) = position_index {
                    if pos > 0 && results.len() >= pos {
                        results = vec![results.swap_remove(pos - 1)];
                    } else if !results.is_empty() {
                        results.clear();
                    }
                }
                return Ok((results, s));
            }
        }
        log::info!("[XPath Fallback] ✗ [fast] ControlView not found — strict mode, returning empty");
        return Ok((vec![], vec![]));
    }
    
    if is_descendant {
        // ── Descendant XPath (//...): prioritize content root ──
        // Content root has far fewer nodes than window root,
        // so descendant search is much faster from content root.
        //
        // IMPORTANT: For descendant XPaths, uiauto-xpath (find_by_xpath_detailed)
        // can hang on large Chrome/WebView subtrees (thousands of nodes).
        // We check for WebView child HWNDs to decide whether it's safe:
        // - WebView detected → skip find_by_xpath_detailed (use raw walk)
        // - No WebView → safe to use find_by_xpath_detailed (Qt/native trees are small)
        
        // Check time budget before expensive strategies
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        // Step 1: Try content root first (fast path — smaller subtree)
        if let Some(content_root) = find_content_root(auto, window) {
            log::info!("[XPath Fallback] //XPath — Step 1: content root (fast)");
            if let Ok((r, s)) = find_by_xpath_detailed(auto, &content_root, xpath, None) {
                if !r.is_empty() {
                    log::info!("[XPath Fallback] ✓ Step 1: Found {} results from content root ({}ms)", 
                        r.len(), fallback_start.elapsed().as_millis());
                    return record_and_return(CompiledStrategy::DescendantContentRoot, r, s, xpath, window, position_index, &fallback_start);
                }
            }
        }
        
        // Step 2: Try from window root.
        // Check for WebView child HWNDs — if present, uiauto-xpath may hang,
        // so use find_by_xpath_raw_descendants instead.
        let has_webview_children = if let Ok(handle) = window.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
            child_hwnds.iter().any(|ch| {
                if let Ok(child_elem) = auto.element_from_handle((*ch).into()) {
                    let class = child_elem.get_classname().unwrap_or_default();
                    is_webview_class(&class)
                } else {
                    false
                }
            })
        } else {
            false
        };
        
        if has_webview_children {
            // WebView detected — skip uiauto-xpath to avoid 30s+ timeout
            log::info!("[XPath Fallback] //XPath — Step 2: raw descendants from window root (WebView detected, skipping uiauto-xpath)");
            if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, window, xpath) {
                if !r.is_empty() {
                    log::info!("[XPath Fallback] ✓ Step 2: Found {} results via raw descendants ({}ms)", 
                        r.len(), fallback_start.elapsed().as_millis());
                    return record_and_return(CompiledStrategy::DescendantRawWalk, r, s, xpath, window, position_index, &fallback_start);
                }
            }
        } else {
            // No WebView — safe to use uiauto-xpath (Qt/native trees won't hang)
            log::info!("[XPath Fallback] //XPath — Step 2: uiauto-xpath from window root (no WebView detected)");
            if let Ok((r, s)) = find_by_xpath_detailed(auto, window, xpath, None) {
                if !r.is_empty() {
                    log::info!("[XPath Fallback] ✓ Step 2: Found {} results via uiauto-xpath ({}ms)", 
                        r.len(), fallback_start.elapsed().as_millis());
                    return record_and_return(CompiledStrategy::DescendantWindowRoot, r, s, xpath, window, position_index, &fallback_start);
                }
            }
            // Fallback: try raw descendants
            if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, window, xpath) {
                if !r.is_empty() {
                    log::info!("[XPath Fallback] ✓ Step 2 (fallback): Found {} results via raw descendants ({}ms)", 
                        r.len(), fallback_start.elapsed().as_millis());
                    return record_and_return(CompiledStrategy::DescendantRawWalk, r, s, xpath, window, position_index, &fallback_start);
                }
            }
        }
        
        // Step 3: EnumChildWindows — try child HWNDs
        // For non-WebView child HWNDs, try find_by_xpath_detailed first (faster & more reliable).
        // For WebView child HWNDs, use find_by_xpath_raw_descendants to avoid hanging.
        if let Ok(handle) = window.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
            log::info!("[XPath Fallback] //XPath — Step 3: trying {} child HWNDs", child_hwnds.len());
            for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    let child_class = child_elem.get_classname().unwrap_or_default();
                    let is_webview = is_webview_class(&child_class);
                    
                    if !is_webview {
                        // Non-WebView child (e.g., QWidget) — try uiauto-xpath first
                        if let Ok((r, s)) = find_by_xpath_detailed(auto, &child_elem, xpath, None) {
                            if !r.is_empty() {
                                log::info!("[XPath Fallback] ✓ Step 3: Found {} from child HWND[{}] via uiauto-xpath ({}ms)",
                                    r.len(), idx, fallback_start.elapsed().as_millis());
                                return record_and_return(CompiledStrategy::DescendantChildHwnd(idx), r, s, xpath, window, position_index, &fallback_start);
                            }
                        }
                    }
                    // Fallback / WebView: raw tree walk
                    if let Ok((r, s)) = find_by_xpath_raw_descendants(auto, &child_elem, xpath) {
                        if !r.is_empty() {
                            log::info!("[XPath Fallback] ✓ Step 3: Found {} from child HWND[{}] via raw walk ({}ms)",
                                r.len(), idx, fallback_start.elapsed().as_millis());
                            return record_and_return(CompiledStrategy::DescendantChildHwnd(idx), r, s, xpath, window, position_index, &fallback_start);
                        }
                    }
                }
            }
        }
        
        log::info!("[XPath Fallback] All //XPath fallbacks exhausted ({}ms)", 
            fallback_start.elapsed().as_millis());
        Ok((vec![], vec![]))
    } else {
        // ── Absolute XPath (/...): optimized multi-strategy approach ──

        // ═══════════════════════════════════════════════════════════════════
        // Walker Hint: Analyze first XPath step to determine which strategy to try first.
        // This avoids the most expensive fallback: trying ControlViewWalker on elements
        // that only exist in the RawView (e.g., Chrome_Widget in WeChat).
        //
        // Key insight: If the first XPath step references a WebView class
        // (Chrome_Widget*, WRY_WEBVIEW*, etc.), Strategy 1 (ControlViewWalker/uiauto-xpath)
        // will ALWAYS fail because these elements are filtered from the ControlView.
        // Skip Strategy 1 entirely and go directly to RawView BFS.
        //
        // Similarly, if the first step has FrameworkId='Chrome', the element
        // likely needs RawView or ChildHwnd search.
        // ═══════════════════════════════════════════════════════════════════
        let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        let first_step_parsed = if !xpath_parts.is_empty() {
            Some(parse_xpath_step(xpath_parts[0]))
        } else {
            None
        };
        
        // Check if first step references a WebView class name
        let first_step_is_webview = first_step_parsed.as_ref().map_or(false, |parsed| {
            // Check ClassName predicates
            let has_webview_class = parsed.required_props.iter().any(|(k, v)| {
                k == "ClassName" && is_webview_class(v)
            }) || parsed.require_starts_with.iter().any(|(k, v)| {
                k == "ClassName" && is_webview_class(v)
            });
            // Check FrameworkId=Chrome
            let has_chrome_fwid = parsed.required_props.iter().any(|(k, v)| {
                k == "FrameworkId" && (v.eq_ignore_ascii_case("Chrome") || v.eq_ignore_ascii_case("WebView"))
            });
            has_webview_class || has_chrome_fwid
        });
        
        // Strategy 1: Try from window root (ControlViewWalker/uiauto-xpath)
        // SKIP if first step is WebView — ControlViewWalker can't see these elements.
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 1: SKIPPED (first step has WebView class, ControlViewWalker won't find it)");
        } else {
            log::info!("[XPath Fallback] /XPath — Strategy 1: window root (primary)");
            let (results, segments) = find_by_xpath_detailed(auto, window, xpath, None)?;
            if !results.is_empty() {
                log::info!("[XPath Fallback] ✓ Strategy 1: Found {} from window root ({}ms)", 
                    results.len(), fallback_start.elapsed().as_millis());
                return record_and_return(CompiledStrategy::ControlViewDirect, results, segments, xpath, window, position_index, &fallback_start);
            }
        }
        
        // Strategy 1.5: RawViewWalker BFS from window root.
        // ControlViewWalker (used by uiauto-xpath) cannot see elements filtered from
        // the control view, such as Chrome_Widget Pane in WeChat. Use RawViewWalker
        // to walk the raw tree, find elements matching the first XPath step, then
        // try the remaining path from each match.
        //
        // SKIP for WebView classes: WebView elements (Chrome_Widget, etc.) live under
        // child HWNDs, NOT as direct raw-tree children of the window root. The BFS
        // would waste ~200ms searching the window's raw tree and find nothing.
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 1.5: SKIPPED (WebView elements are under child HWNDs, not window root)");
        } else if let Some(ref first_parsed) = first_step_parsed {
            if !xpath_parts.is_empty() {
                log::info!("[XPath Fallback] /XPath — Strategy 1.5: RawViewWalker BFS from window root");
                if let Ok(raw_walker) = auto.get_raw_view_walker() {
                    let first_step_end = find_first_step_end(xpath);
                    let remaining_after_first = &xpath[first_step_end..];

                    // BFS from window root using RawViewWalker, max depth 8
                    let mut queue: Vec<(UIElement, u32)> = vec![(window.clone(), 0)];
                    let mut visited: HashSet<Vec<i32>> = HashSet::new();
                    if let Some(rid) = runtime_id_key(window) { visited.insert(rid); }

                    while let Some((elem, depth)) = queue.pop() {
                        if depth > 8 { continue; }

                        // Check this element's children
                        let mut child = raw_walker.get_first_child(&elem).ok();
                        while let Some(c) = child {
                            if let Some(rid) = runtime_id_key(&c) {
                                if !visited.insert(rid) {
                                    child = raw_walker.get_next_sibling(&c).ok();
                                    continue;
                                }
                            }

                            if element_matches_parsed_step(&c, &first_parsed) {
                                log::info!("[XPath Fallback] Strategy 1.5: found first-step match at depth {}", depth + 1);
                                if let Some(result) = try_remaining_from_match(
                                    auto, &c, remaining_after_first, &xpath_parts,
                                    &fallback_start, "1.5",
                                ) {
                                    let (r, s) = result;
                                    return record_and_return(CompiledStrategy::RawViewBfs, r, s, xpath, window, position_index, &fallback_start);
                                }
                            }

                            // Also enqueue for deeper BFS
                            queue.push((c.clone(), depth + 1));
                            child = raw_walker.get_next_sibling(&c).ok();
                        }
                    }
                    log::info!("[XPath Fallback] Strategy 1.5: no match found via RawViewWalker BFS");
                }
            }
        }
        
        // Strategy 2: Try content root if available (handles embedded WebView)
        // SKIP for WebView classes: content root is INSIDE the WebView, but we're
        // looking for the WebView container itself (e.g., Chrome_Widget Pane).
        // Content root can't help find the container — it's below it.
        if first_step_is_webview {
            log::info!("[XPath Fallback] /XPath — Strategy 2: SKIPPED (looking for WebView container, content root is below it)");
        } else {
            log::info!("[XPath Fallback] /XPath — Strategy 2: trying content root...");
            if let Some(content_root) = find_content_root(auto, window) {
                // Try direct path from content root
                if let Ok((r2, s2)) = find_by_xpath_detailed(auto, &content_root, xpath, None) {
                    if !r2.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2a: Found {} from content root ({}ms)", 
                            r2.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ContentRoot, r2, s2, xpath, window, position_index, &fallback_start);
                    }
                }
                
                // Try as descendant from content root
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                log::info!("[XPath Fallback] /XPath — Strategy 2b: content root descendant");
                if let Ok((r3, s3)) = find_by_xpath_detailed(auto, &content_root, &desc_xpath, None) {
                    if !r3.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2b: Found {} from content root desc ({}ms)", 
                            r3.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ContentRoot, r3, s3, xpath, window, position_index, &fallback_start);
                    }
                }
            }
        }
        
        // Strategy 2.5: Use FindAll(TreeScope_Descendants) on the window to search the RAW tree.
        // This is critical for apps like WeChat where Chrome_Widget Pane exists in the raw tree
        // but is filtered out by ControlViewWalker (used by uiauto-xpath's children() method).
        // The capture uses RawViewWalker/FindAll(Subtree), so the XPath was generated against the raw tree,
        // but validation uses ControlViewWalker which can't see these elements.
        //
        // HEURISTIC SKIP: If the first XPath step references a WebView class (e.g., Chrome_Widget),
        // WebView elements are NOT direct raw-tree children of the window — they live under a
        // child HWND. Strategy 2.5's BFS would waste ~200ms finding nothing, so skip to 2.7.
        // (Reuses first_step_is_webview computed at the top of the /XPath branch.)
        {
            if first_step_is_webview {
                log::info!("[XPath Fallback] /XPath — Skipping Strategy 2.5: first step has WebView class, going directly to 2.7");
            } else {
                log::info!("[XPath Fallback] /XPath — Strategy 2.5: FindAll(Descendants) raw tree search");
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r25, s25)) = find_by_xpath_raw_descendants(auto, window, &desc_xpath) {
                    if !r25.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 2.5: Found {} via raw descendant search ({}ms)", 
                            r25.len(), fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::FindAllDescendants, r25, s25, xpath, window, position_index, &fallback_start);
                    }
                }
            }
        }
        
        // Strategy 2.7: Search child HWNDs via EnumChildWindows.
        // Critical for apps like WeChat where Chrome_Widget Pane is under a child HWND
        // that's not visible as a direct child in the UIA tree from the main window.
        // The UIA tree can have an asymmetry: GetParentElement(child) → Window works,
        // but GetFirstChildElement(Window) → child doesn't return it.
        // EnumChildWindows bypasses this by enumerating Win32 child HWNDs directly.
        
        // Check time budget before expensive child HWND + sibling search strategies
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted before Strategy 2.7 ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        log::info!("[XPath Fallback] /XPath — Strategy 2.7: child HWND search via EnumChildWindows");
        
        /// Find the byte offset where the first XPath step ends in the original string.
        /// E.g., for "/Pane[...]/Document[...]//Group[...]", returns the position right after "Pane[...]".
        /// This preserves `//` axis markers that would be lost by split/join.
        fn find_first_step_end(xpath: &str) -> usize {
            let bytes = xpath.as_bytes();
            let mut i = 0;
            // Skip leading slashes
            while i < bytes.len() && bytes[i] == b'/' {
                i += 1;
            }
            // Now at start of first step content
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
                            // End of first step (start of next / or //)
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
        /// Returns per-segment validation results consistent with other strategies.
        ///
        /// `remaining_xpath` is the XPath after the first step, preserving `//` axis markers.
        /// `xpath_parts` is the split step list (for segment validation results only).
        ///
        /// ## Fast path for `//` descendant searches
        ///
        /// When the remaining XPath contains `//` (descendant-or-self axis), the
        /// uiauto-xpath evaluation can hang on very large subtrees (e.g., Chrome/WebView
        /// with thousands of nodes). To avoid 30-second COM worker timeouts:
        ///
        /// 1. **FindAll fast path**: If the final `//` step has simple equality predicates
        ///    (like `//Text[@Name='...']`), use the native UIA `FindAll(TreeScope_Subtree)`
        ///    API which is much faster than XPath evaluation.
        /// 2. **Skip uiauto-xpath for `//`**: If the remaining XPath contains `//`, skip
        ///    `find_by_xpath_detailed` entirely to prevent hanging, and rely on the
        ///    FindAll fast path + raw tree walk fallback.
        fn try_remaining_from_match(
            auto: &UIAutomation,
            match_elem: &UIElement,
            remaining_xpath: &str,
            xpath_parts: &[&str],
            fallback_start: &std::time::Instant,
            strategy_label: &str,
        ) -> Option<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
            // If remaining path is empty, the matched element IS the result
            if remaining_xpath.is_empty() || remaining_xpath == "/" {
                let duration_ms = fallback_start.elapsed().as_millis() as u64;
                log::info!("[XPath Fallback] ✓ Strategy {}: Found element ({}ms)", strategy_label, duration_ms);
                let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                    SegmentValidationResult {
                        segment_index: i,
                        segment_text: step.to_string(),
                        matched: true,
                        match_count: if i == xpath_parts.len() - 1 { 1 } else { 0 },
                        duration_ms: if i == xpath_parts.len() - 1 { duration_ms } else { 0 },
                        predicate_failures: Vec::new(),
                    }
                }).collect();
                return Some((vec![match_elem.clone()], segments));
            }
            
            log::info!("[XPath Fallback] Strategy {}: trying remaining XPath from matched element: {}", strategy_label, remaining_xpath);

            // ═══════════════════════════════════════════════════════════════════════
            // FAST PATH: Use native UIA FindAll for descendant searches
            //
            // When the remaining XPath contains `//`, the uiauto-xpath evaluation
            // can hang on large Chrome/WebView subtrees (30s+ timeout). Instead,
            // use the native UIA FindAll API which is much faster.
            // ═══════════════════════════════════════════════════════════════════════
            let has_descendant_axis = remaining_xpath.contains("//");
            if has_descendant_axis {
                if let Some(result) = findall_descendants_fast(auto, match_elem, remaining_xpath, xpath_parts, fallback_start, strategy_label) {
                    return Some(result);
                }
                // FindAll fast path didn't find results — do NOT fall through to
                // find_by_xpath_detailed (it would hang on large subtrees).
                // Go directly to raw tree walk.
                log::info!("[XPath Fallback] Strategy {}: FindAll fast path found nothing, skipping uiauto-xpath to avoid timeout", strategy_label);
            } else {
                // No `//` descendant axis — uiauto-xpath is safe (child-only traversal)
                if let Ok((matches, segments)) = find_by_xpath_detailed(auto, match_elem, remaining_xpath, None) {
                    if !matches.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy {}: Found {} from subtree ({}ms)",
                            strategy_label, matches.len(), fallback_start.elapsed().as_millis());
                        let mut all_segments = vec![SegmentValidationResult {
                            segment_index: 0,
                            segment_text: xpath_parts.first().unwrap_or(&"").to_string(),
                            matched: true,
                            match_count: 1,
                            duration_ms: 0,
                            predicate_failures: Vec::new(),
                        }];
                        for mut s in segments {
                            s.segment_index += 1;
                            all_segments.push(s);
                        }
                        return Some((matches, all_segments));
                    }
                }
            }
            
            // Fallback: raw tree walk for remaining steps
            if let Ok(raw_walker) = auto.get_raw_view_walker() {
                let remaining_parts: Vec<&str> = remaining_xpath.split('/').filter(|s| !s.is_empty()).collect();
                if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, match_elem, &remaining_parts) {
                    if !matches.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy {}: Found {} via raw walk ({}ms)",
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

        /// Fast path for descendant searches using native UIA `FindAll` API.
        ///
        /// When the remaining XPath contains `//` (descendant-or-self axis), e.g.:
        ///   `/Document[...]/Group[...]//Text[@Name='...']`
        ///
        /// uiauto-xpath can hang on large Chrome/WebView subtrees. This function
        /// extracts the final `//` step's conditions and uses `FindAll(TreeScope_Subtree)`
        /// which is a native UIA API call — much faster than interpreted XPath evaluation.
        ///
        /// Only handles `//ElementType[@Prop='value']` patterns with simple equality
        /// predicates. Returns `None` for complex predicates (starts-with, contains, etc.)
        /// or if FindAll fails.
        fn findall_descendants_fast(
            auto: &UIAutomation,
            match_elem: &UIElement,
            remaining_xpath: &str,
            xpath_parts: &[&str],
            fallback_start: &std::time::Instant,
            strategy_label: &str,
        ) -> Option<(Vec<UIElement>, Vec<SegmentValidationResult>)> {

            // Find the last `//` in the remaining XPath and extract the final step
            let last_desc_idx = remaining_xpath.rfind("//")?;
            let last_step_str = &remaining_xpath[last_desc_idx + 2..]; // skip "//"
            
            // Parse the final step to extract type and equality predicates
            let parsed = parse_xpath_step(last_step_str);
            
            // Only use FindAll fast path when we have simple equality predicates
            // (no starts-with, contains, matches — those can't be expressed as UIA conditions)
            if !parsed.require_starts_with.is_empty()
                || !parsed.require_contains.is_empty()
                || !parsed.require_matches.is_empty()
            {
                log::info!("[XPath Fallback] Strategy {}: FindAll fast path skipped — complex predicates in final step: {}",
                    strategy_label, last_step_str);
                return None;
            }

            // Navigate to the parent of the final `//` step first (prefix steps)
            // e.g., for `/Document[...]/Group[...]//Text[@Name='...']`
            //   prefix = `/Document[...]/Group[...]`
            //   We need to navigate to the Group element first, then use FindAll from there
            let prefix_xpath = &remaining_xpath[..last_desc_idx];
            let search_root = if prefix_xpath.is_empty() || prefix_xpath == "/" {
                // No prefix — search from match_elem directly
                match_elem.clone()
            } else {
                // Navigate prefix steps using uiauto-xpath (should be fast — child steps only)
                log::info!("[XPath Fallback] Strategy {}: FindAll fast path — navigating prefix: {}",
                    strategy_label, prefix_xpath);
                match find_by_xpath_detailed(auto, match_elem, prefix_xpath, None) {
                    Ok((elements, _)) => {
                        if let Some(first) = elements.into_iter().next() {
                            first
                        } else {
                            log::info!("[XPath Fallback] Strategy {}: FindAll fast path — prefix found no elements", strategy_label);
                            return None;
                        }
                    }
                    Err(e) => {
                        log::info!("[XPath Fallback] Strategy {}: FindAll fast path — prefix navigation failed: {}", strategy_label, e);
                        return None;
                    }
                }
            };

            // Build UIA conditions from parsed predicates
            let mut conditions: Vec<UICondition> = Vec::new();

            // Add ControlType condition if type_name is specified
            if let Some(ref type_name) = parsed.type_name {
                if let Some(ct_id) = control_type_name_to_id(type_name) {
                    let variant = Variant::from(ct_id);
                    if let Ok(cond) = auto.create_property_condition(UIProperty::ControlType, variant, None) {
                        conditions.push(cond);
                    }
                }
            }

            // Add equality predicate conditions (@Name='...', @AutomationId='...', @FrameworkId='...', etc.)
            // Only add conditions for properties that have a UIA property ID mapping.
            for (key, value) in &parsed.required_props {
                let prop: UIProperty = match key.as_str() {
                    "Name" => UIProperty::Name,
                    "AutomationId" => UIProperty::AutomationId,
                    "FrameworkId" => UIProperty::FrameworkId,
                    "ClassName" => UIProperty::ClassName,
                    "ControlType" => {
                        // ControlType already handled above via type_name
                        continue;
                    }
                    _ => {
                        // Unknown property — can't create UIA condition
                        log::debug!("[FindAll fast path] Skipping unknown property: @{}", key);
                        continue;
                    }
                };

                let variant = Variant::from(value.clone());
                if let Ok(cond) = auto.create_property_condition(prop, variant, None) {
                    conditions.push(cond);
                }
            }

            // Combine conditions
            let final_condition = match conditions.len() {
                0 => {
                    log::info!("[XPath Fallback] Strategy {}: FindAll fast path — no conditions built, skipping", strategy_label);
                    return None;
                }
                1 => conditions.remove(0),
                2 => {
                    let cond2 = conditions.remove(1);
                    let cond1 = conditions.remove(0);
                    auto.create_and_condition(cond1.clone(), cond2).unwrap_or(cond1)
                }
                _ => {
                    // Chain create_and_condition for 3+ conditions
                    let mut iter = conditions.into_iter();
                    let first = iter.next().expect("at least one condition");
                    let mut combined: Option<UICondition> = Some(first);
                    for cond in iter {
                        let current = match combined.take() {
                            Some(c) => c,
                            None => break,
                        };
                        match auto.create_and_condition(current, cond) {
                            Ok(c) => combined = Some(c),
                            Err(_) => break,
                        }
                    }
                    combined.unwrap_or_else(|| auto.create_true_condition().expect("at least one condition"))
                }
            };

            // Execute FindAll
            let findall_start = std::time::Instant::now();
            log::info!("[XPath Fallback] Strategy {}: FindAll fast path — executing on subtree (step: {})",
                strategy_label, last_step_str);
            
            match search_root.find_all(TreeScope::Subtree, &final_condition) {
                Ok(elements) => {
                    let count = elements.len();
                    let findall_ms = findall_start.elapsed().as_millis();
                    log::info!("[XPath Fallback] Strategy {}: FindAll fast path — found {} elements ({}ms)",
                        strategy_label, count, findall_ms);

                    if count == 0 {
                        return None;
                    }

                    let matches = elements;

                    log::info!("[XPath Fallback] ✓ Strategy {}: FindAll fast path found {} elements ({}ms total)",
                        strategy_label, matches.len(), fallback_start.elapsed().as_millis());

                    // Build segment validation results
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

                    Some((matches, segments))
                }
                Err(e) => {
                    log::warn!("[XPath Fallback] Strategy {}: FindAll fast path failed: {:?}", strategy_label, e);
                    None
                }
            }
        }
        
        // Compute the remaining XPath after the first step, preserving // axis markers
        let first_step_end = find_first_step_end(xpath);
        let remaining_after_first = &xpath[first_step_end..];
        
        if let Ok(handle) = window.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            let child_hwnds = enum_child_hwnds(HWND(raw_handle.0));
            log::info!("[Strategy 2.7] Found {} child HWNDs under window", child_hwnds.len());

            let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
            let xpath_parts: Vec<&str> = desc_xpath.split('/').filter(|s| !s.is_empty()).collect();

            for (idx, child_hwnd) in child_hwnds.iter().enumerate() {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    let child_class = child_elem.get_classname().unwrap_or_default();
                    let child_ct = child_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                    let child_pid = child_elem.get_process_id().unwrap_or(0);
                    log::info!("[Strategy 2.7]   child_hwnd[{}]: hwnd=0x{:X} type='{}' class='{}' pid={}",
                        idx, child_hwnd.0 as usize, child_ct, child_class, child_pid);
                    
                    // Try: if this child element itself matches the first step, search from it
                    if !xpath_parts.is_empty() {
                        let first_parsed = parse_xpath_step(xpath_parts[0]);
                        if element_matches_parsed_step(&child_elem, &first_parsed) {
                            log::info!("[Strategy 2.7]   ✓ child HWND matches first step!");
                            if let Some(result) = try_remaining_from_match(auto, &child_elem, remaining_after_first, &xpath_parts, &fallback_start, "2.7a") {
                                let (r, s) = result;
                                return record_and_return(CompiledStrategy::ChildHwndEnum(idx), r, s, xpath, window, position_index, &fallback_start);
                            }
                        }
                        
                        // Strategy 2.7c: Try uiauto-xpath descendant search from this child HWND.
                        // For non-WebView children (like QWidget), uiauto-xpath can reliably
                        // traverse the full subtree. For WebView children, skip to avoid hanging.
                        let is_webview = is_webview_class(&child_class);
                        if !is_webview {
                            log::info!("[Strategy 2.7]   Trying uiauto-xpath descendant search from child HWND[{}]", idx);
                            if let Ok((r, s)) = find_by_xpath_detailed(auto, &child_elem, &desc_xpath, None) {
                                if !r.is_empty() {
                                    log::info!("[Strategy 2.7]   ✓ Found {} via uiauto-xpath from child HWND[{}] ({}ms)",
                                        r.len(), idx, fallback_start.elapsed().as_millis());
                                    return record_and_return(CompiledStrategy::ChildHwndEnum(idx), r, s, xpath, window, position_index, &fallback_start);
                                }
                            }
                        }
                        
                        // Also try: search inside this child HWND's subtree for the first step
                        if let Ok(raw_walker) = auto.get_raw_view_walker() {
                            let first_parsed = parse_xpath_step(xpath_parts[0]);
                            let mut sub_match = raw_walker.get_first_child(&child_elem).ok();
                            while let Some(sub) = sub_match {
                                if element_matches_parsed_step(&sub, &first_parsed) {
                                    log::info!("[Strategy 2.7]   ✓ Found first-step match inside child HWND!");
                                    if let Some(result) = try_remaining_from_match(auto, &sub, remaining_after_first, &xpath_parts, &fallback_start, "2.7b") {
                                        let (r, s) = result;
                                        return record_and_return(CompiledStrategy::ChildHwndEnum(idx), r, s, xpath, window, position_index, &fallback_start);
                                    }
                                }
                                sub_match = raw_walker.get_next_sibling(&sub).ok();
                            }
                        }
                    }
                }
            }
        } else {
            log::info!("[Strategy 2.7] Could not get window HWND, skipping");
        }
        
        // Strategy 3: Try sibling windows of the same process AND child processes (handles multi-process apps like WeChat)
        // Check time budget — sibling search is expensive and can hang on WebView windows
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted before Strategy 3 ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        log::info!("[XPath Fallback] /XPath — Strategy 3: searching sibling windows and child processes...");
        if let Ok(siblings) = find_sibling_windows_same_process(auto, window) {
            log::info!("[XPath Fallback] Found {} sibling windows, trying XPath on each", siblings.len());
            for (idx, sibling) in siblings.iter().enumerate() {
                // Check time budget per sibling
                if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
                    log::info!("[XPath Fallback] Time budget exhausted at sibling[{}] ({}ms)", idx, fallback_start.elapsed().as_millis());
                    break;
                }
                
                // Skip WebView siblings entirely — even /Document absolute path can take
                // 1286ms+ on Chrome windows, and descendant search is even worse.
                // The target element will be found from the correct window in the main loop,
                // not from a sibling fallback.
                let sibling_class = sibling.get_classname().unwrap_or_default();
                if is_webview_class(&sibling_class) {
                    log::info!("[Strategy 3] Skipping WebView sibling[{}] class='{}'", idx, sibling_class);
                    continue;
                }
                
                // Try absolute XPath first
                if let Ok((r, s)) = find_by_xpath_detailed(auto, sibling, xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 3: Found {} from sibling window {} ({}ms)", 
                            r.len(), idx + 1, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::SiblingWindow, r, s, xpath, window, position_index, &fallback_start);
                    }
                }
                // Also try descendant search from sibling (safe for non-WebView)
                let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
                if let Ok((r, s)) = find_by_xpath_detailed(auto, sibling, &desc_xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 3 (desc): Found {} from sibling window {} ({}ms)", 
                            r.len(), idx + 1, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::SiblingWindow, r, s, xpath, window, position_index, &fallback_start);
                    }
                }
            }
        }
        
        // Strategy 3b: Try to find child process windows (for apps like WeChat with separate WebView processes)
        if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
            log::info!("[XPath Fallback] Time budget exhausted before Strategy 3b ({}ms), returning empty", fallback_start.elapsed().as_millis());
            return Ok((vec![], vec![]));
        }
        
        log::info!("[XPath Fallback] /XPath — Strategy 3b: searching child process windows...");
        if let Ok(child_windows) = find_child_process_windows(auto, window) {
            log::info!("[XPath Fallback] Found {} child process windows, trying XPath on each", child_windows.len());
            
            // Also try descendant search from child windows
            let desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
            
            for (idx, child_win) in child_windows.iter().enumerate() {
                // Check time budget per child window
                if fallback_start.elapsed().as_millis() > XPATH_FALLBACK_BUDGET_MS {
                    log::info!("[XPath Fallback] Time budget exhausted at child window[{}] ({}ms)", idx, fallback_start.elapsed().as_millis());
                    break;
                }
                
                // Get window info for debugging
                let child_pid = child_win.get_process_id().unwrap_or(0);
                let child_name = child_win.get_name().unwrap_or_default();
                let child_class = child_win.get_classname().unwrap_or_default();
                let child_is_webview = is_webview_class(&child_class);
                log::info!("[Strategy 3b] Trying window {}: PID={}, Class='{}', Name='{}', WebView={}", 
                    idx + 1, child_pid, child_class, child_name, child_is_webview);
                
                // Skip WebView child windows entirely — same reason as Strategy 3
                if child_is_webview {
                    log::info!("[Strategy 3b] Skipping WebView child window[{}]", idx);
                    continue;
                }
                
                // Try absolute path first
                if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 3b: Found {} from child process window {} (absolute path, {}ms)", 
                            r.len(), idx + 1, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildProcessWindow, r, s, xpath, window, position_index, &fallback_start);
                    }
                }
                
                // Try descendant search as fallback — ONLY for non-WebView windows
                if let Ok((r, s)) = find_by_xpath_detailed(auto, child_win, &desc_xpath, None) {
                    if !r.is_empty() {
                        log::info!("[XPath Fallback] ✓ Strategy 3b: Found {} from child process window {} (descendant path, {}ms)", 
                            r.len(), idx + 1, fallback_start.elapsed().as_millis());
                        return record_and_return(CompiledStrategy::ChildProcessWindow, r, s, xpath, window, position_index, &fallback_start);
                    }
                }
            }
        }
        
        // Strategy 4: Desktop root descendant search — DISABLED by default.
        //
        // Rationale: All searches in this system are window-scoped — we always locate the
        // target window first, then search within it. Desktop root search is fundamentally
        // incompatible with this design:
        //   1. It searches the entire OS UI tree, which can take minutes on large trees
        //      (e.g., Chrome/WebView with thousands of nodes).
        //   2. The "timeout" check is post-hoc — it only checks elapsed AFTER the query
        //      completes, so it doesn't actually prevent hangs.
        //   3. If Strategies 1-3b couldn't find the element within the correct window,
        //      a global search is extremely unlikely to help — the element probably
        //      doesn't exist anymore or the XPath is stale.
        //   4. Compass (relative) XPaths use /.., /preceding-sibling::, /following-sibling::
        //      which are meaningless when searched from Desktop root.
        //
        // To explicitly enable (for rare edge cases), set environment variable:
        //   ENABLE_DESKTOP_SEARCH=1
        let desktop_search_enabled = std::env::var("ENABLE_DESKTOP_SEARCH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        
        if !desktop_search_enabled {
            log::info!("[XPath Fallback] /XPath — Strategy 4: SKIPPED (Desktop root search disabled by default, set ENABLE_DESKTOP_SEARCH=1 to enable)");
        } else {
            log::info!("[XPath Fallback] /XPath — Strategy 4: Desktop root descendant (explicitly enabled via ENABLE_DESKTOP_SEARCH=1)");
            let desktop_desc_xpath = format!("//{}", xpath.trim_start_matches('/'));
            if let Ok(desktop) = auto.get_root_element() {
                let strategy4_start = std::time::Instant::now();
                let timeout_ms = 2000; // 2 second timeout for Desktop search
                
                if let Ok((r4, s4)) = find_by_xpath_detailed(auto, &desktop, &desktop_desc_xpath, None) {
                    let elapsed = strategy4_start.elapsed().as_millis();
                    if !r4.is_empty() && elapsed < timeout_ms {
                        log::info!("[XPath Fallback] ✓ Strategy 4: Found {} from Desktop root desc ({}ms)", 
                            r4.len(), fallback_start.elapsed().as_millis());
                        // Strategy 4 is a last resort, not cached by default
                        return record_and_return(CompiledStrategy::DescendantWindowRoot, r4, s4, xpath, window, position_index, &fallback_start);
                    } else if elapsed >= timeout_ms {
                        log::warn!("[XPath Fallback] Strategy 4 timed out after {}ms, skipping result", elapsed);
                    }
                }
            }
        }
        
        log::info!("[XPath Fallback] All /XPath strategies exhausted ({}ms)", 
            fallback_start.elapsed().as_millis());
        Ok((vec![], vec![]))
    }
}

fn cached_raw_view_bfs(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_raw_descendants(auto, window, xpath)
}

fn cached_child_hwnd_search(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    child_idx: usize,
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
    find_by_xpath_raw_descendants(auto, &child_elem, xpath)
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
    find_by_xpath_raw_descendants(auto, &child_elem, xpath)
}

fn has_child_predicate(predicates_str: &str) -> bool {
    let bytes = predicates_str.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(pos) = predicates_str[i..].find("[@") {
            let abs_pos = i + pos;
            // Check if the character before `[` is a word character
            // (indicating a ControlType name, not a logical operator like ' and ')
            if abs_pos > 0 {
                let prev = bytes[abs_pos - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    return true;
                }
            }
            i = abs_pos + 2; // skip past "[@"
        } else {
            break;
        }
    }
    false
}

fn parse_xpath_step(step: &str) -> ParsedXPathStep {
    let (type_name, predicates_str): (Option<String>, &str) = if step.starts_with('[') {
        (None, step)
    } else if let Some(bracket_pos) = step.find('[') {
        (Some(step[..bracket_pos].to_string()), &step[bracket_pos..])
    } else {
        (Some(step.to_string()), "")
    };

    // Detect `or` or `not()` — these cannot be reliably handled by simple
    // key=value matching; clear properties to force uiauto-xpath fallback.
    if predicates_str.contains(" or ") || predicates_str.contains("not(") {
        log::warn!("[parse_xpath_step] Detected 'or'/'not()' in predicates, skipping simple matching for: {}", step);
        return ParsedXPathStep {
            type_name,
            required_props: Vec::new(),
            require_starts_with: Vec::new(),
            require_contains: Vec::new(),
            require_matches: Vec::new(),
        };
    }

    // Detect child element predicates: e.g., `Button[Text[@Name='确认']]`
    // Pattern: inside the outer predicates, there's a ControlType token followed by `[@`.
    if has_child_predicate(predicates_str) {
        log::info!("[parse_xpath_step] Detected child predicate in step, falling back to uiauto-xpath: {}", step);
        return ParsedXPathStep {
            type_name,
            required_props: Vec::new(),
            require_starts_with: Vec::new(),
            require_contains: Vec::new(),
            require_matches: Vec::new(),
        };
    }

    let mut required_props: Vec<(String, String)> = Vec::new();
    let mut require_starts_with: Vec<(String, String)> = Vec::new();
    let mut require_contains: Vec<(String, String)> = Vec::new();
    let mut require_matches: Vec<(String, regex::Regex)> = Vec::new();
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Pre-compiled regexes — avoids re-compiling on every BFS node visit
    static RE_EXACT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"@(\w+)='([^']*)'"#).unwrap()
    });
    static RE_STARTS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"starts-with\(@(\w+),\s*['\"]([^'\"]*)['\"]\)"#).unwrap()
    });
    static RE_CONTAINS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"contains\(@(\w+),\s*['\"]([^'\"]*)['\"]\)"#).unwrap()
    });
    static RE_MATCHES: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?:matches|match)\(@(\w+),\s*['\"]([^'\"]*)['\"]\)"#).unwrap()
    });

    for cap in RE_EXACT.captures_iter(predicates_str) {
        if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
            let k = key.as_str().to_string();
            let v = val.as_str().to_string();
            if seen_keys.contains(&k) {
                log::warn!("[parse_xpath_step] Duplicate key '{}' in step '{}', keeping last value '{}'", k, step, v);
                if let Some(entry) = required_props.iter_mut().find(|(ek, _)| *ek == k) {
                    entry.1 = v.clone();
                }
            } else {
                seen_keys.insert(k.clone());
                required_props.push((k, v));
            }
        }
    }
    for cap in RE_STARTS.captures_iter(predicates_str) {
        if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
            require_starts_with.push((key.as_str().to_string(), val.as_str().to_string()));
        }
    }
    for cap in RE_CONTAINS.captures_iter(predicates_str) {
        if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
            require_contains.push((key.as_str().to_string(), val.as_str().to_string()));
        }
    }
    for cap in RE_MATCHES.captures_iter(predicates_str) {
        if let (Some(key), Some(pattern)) = (cap.get(1), cap.get(2)) {
            match regex::Regex::new(pattern.as_str()) {
                Ok(compiled) => require_matches.push((key.as_str().to_string(), compiled)),
                Err(e) => log::warn!("[parse_xpath_step] Invalid regex '{}': {}", pattern.as_str(), e),
            }
        }
    }

    ParsedXPathStep { type_name, required_props, require_starts_with, require_contains, require_matches }
}

fn get_uia_property_for_xpath(elem: &UIElement, key: &str) -> String {
    match key {
        "Name" => elem.get_name().unwrap_or_default(),
        "ClassName" => elem.get_classname().unwrap_or_default(),
        "AutomationId" => elem.get_automation_id().unwrap_or_default(),
        "FrameworkId" => elem.get_framework_id().unwrap_or_default(),
        "HelpText" => elem.get_help_text().unwrap_or_default(),
        _ => String::new(),
    }
}

fn element_matches_parsed_step(elem: &UIElement, step: &ParsedXPathStep) -> bool {
    // Check control type
    if let Some(ref type_name) = step.type_name {
        let elem_ct = elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
        if elem_ct != *type_name {
            return false;
        }
    }

    // Check exact property matches
    for (key, val) in &step.required_props {
        let actual = get_uia_property_for_xpath(elem, key);
        if actual != *val {
            return false;
        }
    }

    // Check starts-with predicates
    for (key, prefix) in &step.require_starts_with {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.starts_with(prefix) {
            return false;
        }
    }

    // Check contains predicates
    for (key, needle) in &step.require_contains {
        let actual = get_uia_property_for_xpath(elem, key);
        if !actual.contains(needle) {
            return false;
        }
    }

    // Check regex predicates
    for (key, pattern) in &step.require_matches {
        let actual = get_uia_property_for_xpath(elem, key);
        if !pattern.is_match(&actual) {
            return false;
        }
    }

    true
}

fn walk_raw_tree_steps(
    auto: &UIAutomation,
    raw_walker: &UITreeWalker,
    root: &UIElement,
    steps: &[&str],
) -> anyhow::Result<Vec<UIElement>> {
    if steps.is_empty() {
        return Ok(vec![root.clone()]);
    }

    let first_parsed = parse_xpath_step(steps[0]);

    // Find children of root matching the first step
    let mut current_matches: Vec<UIElement> = Vec::new();
    let mut child = raw_walker.get_first_child(root).ok();
    while let Some(c) = child {
        if element_matches_parsed_step(&c, &first_parsed) {
            current_matches.push(c.clone());
        }
        child = raw_walker.get_next_sibling(&c).ok();
    }

    if current_matches.is_empty() {
        return Ok(vec![]);
    }

    // If this is the last step, return the matches
    if steps.len() == 1 {
        return Ok(current_matches);
    }

    // Recurse for remaining steps
    let remaining = &steps[1..];
    let mut all_matches = Vec::new();
    for candidate in &current_matches {
        if let Ok(sub_matches) = walk_raw_tree_steps(auto, raw_walker, candidate, remaining) {
            all_matches.extend(sub_matches);
        }
    }

    Ok(all_matches)
}

fn find_by_xpath_raw_descendants(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let start = Instant::now();

    // Parse XPath steps (skip leading // or /)
    let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
    if xpath_parts.is_empty() {
        return Ok((vec![], vec![]));
    }

    // Get the first step to match
    let first_step = xpath_parts[0];

    // Parse the first step into (control_type, predicates)
    let first_step_parsed = parse_xpath_step(first_step);
    log::info!("[Raw Desc] First step: type={:?}, exact={:?}, starts_with={:?}",
        first_step_parsed.type_name, first_step_parsed.required_props, first_step_parsed.require_starts_with);

    // Use RawViewWalker to find elements matching the first step
    let raw_walker = match auto.get_raw_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Raw Desc] Failed to get RawViewWalker: {}", e);
            return Ok((vec![], vec![]));
        }
    };

    // ── Diagnostic: print raw tree children at depth 1 and 2 (debug builds only) ──
    #[cfg(debug_assertions)]
    {
        let mut diag_count = 0u32;
        let mut d1_child = raw_walker.get_first_child(window).ok();
        while let Some(c) = d1_child {
            let ct = c.get_control_type_raw().map(control_type_name).unwrap_or_default();
            let cn = c.get_classname().unwrap_or_default();
            let nm = c.get_name().unwrap_or_default();
            let fw = c.get_framework_id().unwrap_or_default();
            let pid = c.get_process_id().unwrap_or(0);
            log::info!("[Raw Desc]   raw_depth1[{}]: {} class='{}' name='{}' fw='{}' pid={}",
                diag_count, ct, cn, nm, fw, pid);
            // Print depth-2 children of the first 3 depth-1 elements
            if diag_count < 3 {
                let mut d2_idx = 0u32;
                let mut d2_child = raw_walker.get_first_child(&c).ok();
                while let Some(c2) = d2_child {
                    if d2_idx < 5 {
                        let ct2 = c2.get_control_type_raw().map(control_type_name).unwrap_or_default();
                        let cn2 = c2.get_classname().unwrap_or_default();
                        let fw2 = c2.get_framework_id().unwrap_or_default();
                        log::info!("[Raw Desc]     raw_depth2[{}]: {} class='{}' fw='{}'", d2_idx, ct2, cn2, fw2);
                    }
                    d2_idx += 1;
                    d2_child = raw_walker.get_next_sibling(&c2).ok();
                }
                if d2_idx > 5 {
                    log::info!("[Raw Desc]     ... and {} more depth-2 children", d2_idx - 5);
                }
            }
            diag_count += 1;
            d1_child = raw_walker.get_next_sibling(&c).ok();
        }
        log::info!("[Raw Desc] Window has {} raw children at depth 1", diag_count);
    }

    // Collect raw tree children of the window, then search deeper if needed
    let mut first_step_matches: Vec<UIElement> = Vec::new();

    // BFS: search for first-step matches in the raw tree
    // Depth 8 covers most real-world UI hierarchies (e.g., WeChat's Qt tree can be 7+ levels deep).
    // Previously depth 3 was too shallow, causing elements captured by RawViewWalker to be
    // unreachable during validation/search.
    let mut queue: std::collections::VecDeque<(UIElement, u32)> = std::collections::VecDeque::from(vec![(window.clone(), 0)]);
    let max_depth = 8u32;

    while let Some((elem, depth)) = queue.pop_front() {
        let mut child = raw_walker.get_first_child(&elem).ok();
        while let Some(c) = child {
            // Check if this child matches the first step
            if element_matches_parsed_step(&c, &first_step_parsed) {
                first_step_matches.push(c.clone());
            }
            // Add to queue for deeper search if within depth limit
            if depth + 1 < max_depth {
                queue.push_back((c.clone(), depth + 1));
            }
            child = raw_walker.get_next_sibling(&c).ok();
        }
    }

    log::info!("[Raw Desc] Found {} elements matching first step via RawViewWalker ({}ms)",
        first_step_matches.len(), start.elapsed().as_millis());

    if first_step_matches.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        return Ok((vec![], vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: first_step.to_string(),
            matched: false,
            match_count: 0,
            duration_ms,
            predicate_failures: vec![super::PredicateFailure {
                attr_name: "RawTree".to_string(),
                expected_value: first_step.to_string(),
                actual_value: None,
                reason: "No raw tree element matches this step".to_string(),
            }],
        }]));
    }

    // Build remaining XPath (steps after the first)
    let remaining_parts = &xpath_parts[1..];
    let remaining_xpath = if remaining_parts.is_empty() {
        String::new()
    } else {
        format!("/{}", remaining_parts.join("/"))
    };

    // If no remaining steps, the first-step matches ARE the result
    if remaining_parts.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        let match_count = first_step_matches.len();
        log::info!("[Raw Desc] First step is the last step, returning {} matches ({}ms)",
            match_count, duration_ms);
        return Ok((first_step_matches, vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: xpath.to_string(),
            matched: true,
            match_count,
            duration_ms,
            predicate_failures: Vec::new(),
        }]));
    }

    // Strategy A: Try uiauto-xpath from each first-step match for the remaining XPath
    // This works if ControlViewWalker CAN navigate inside the Chrome fragment
    // (e.g., Document is visible in the control view even though Pane is not)
    //
    // IMPORTANT: Skip uiauto-xpath when remaining_xpath contains `//` (descendant axis).
    // The uiauto-xpath evaluation can hang on large Chrome/WebView subtrees,
    // causing 30-second COM worker timeouts. For descendant searches, go directly
    // to Strategy B (raw tree walk) or use FindAll fast path.
    let remaining_has_descendant = remaining_xpath.contains("//");
    if remaining_has_descendant {
        log::info!("[Raw Desc] Skipping Strategy A (uiauto-xpath): remaining XPath has // descendant axis, using raw walk instead ({}ms)",
            start.elapsed().as_millis());
    } else {
        for candidate in &first_step_matches {
            if let Ok((matches, segments)) = find_by_xpath_detailed(auto, candidate, &remaining_xpath, None) {
                if !matches.is_empty() {
                    log::info!("[Raw Desc] ✓ uiauto-xpath found {} from raw candidate ({}ms)",
                        matches.len(), start.elapsed().as_millis());
                    // Prepend a segment for the first step
                    let mut all_segments = vec![SegmentValidationResult {
                        segment_index: 0,
                        segment_text: first_step.to_string(),
                        matched: true,
                        match_count: 1,
                        duration_ms: 0,
                        predicate_failures: Vec::new(),
                    }];
                    for mut s in segments {
                        s.segment_index += 1;
                        all_segments.push(s);
                    }
                    return Ok((matches, all_segments));
                }
            }
        }
        log::info!("[Raw Desc] uiauto-xpath failed from raw candidates, falling back to full raw tree walk ({}ms)",
            start.elapsed().as_millis());
    }

    // Strategy B: Walk the raw tree manually for ALL remaining steps
    // This handles the case where ControlViewWalker can't navigate inside the Chrome fragment
    let mut all_matches = Vec::new();
    for candidate in &first_step_matches {
        if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, candidate, remaining_parts) {
            if !matches.is_empty() {
                all_matches.extend(matches);
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    log::info!("[Raw Desc] Full raw walk found {} matches ({}ms)", all_matches.len(), duration_ms);

    let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
        SegmentValidationResult {
            segment_index: i,
            segment_text: step.to_string(),
            matched: i < xpath_parts.len() - 1 || !all_matches.is_empty(),
            match_count: if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 },
            duration_ms: 0,
            predicate_failures: Vec::new(),
        }
    }).collect();

    Ok((all_matches, segments))
}

fn find_by_xpath_detailed(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
    visibility_filter: Option<uiauto_xpath::xpath::VisibilityFilter>,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_detailed_impl(auto, root, xpath, visibility_filter, false)
}

fn find_by_xpath_detailed_strict(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_detailed_impl(auto, root, xpath, None, true)
}

fn find_by_xpath_detailed_impl(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
    visibility_filter: Option<uiauto_xpath::xpath::VisibilityFilter>,
    strict: bool,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;

    let total_start = Instant::now();
    info!("[XPath Validation] Executing XPath with uiauto-xpath: {}", xpath);

    // Wrap the root element for uiauto-xpath
    // UiaXPathElement::new requires IUIAutomationElement and IUIAutomation (raw COM types)
    // We extract them from the safe wrappers via Into trait
    let uia_elem = UiaXPathElement::new(root.clone().into(), auto.clone().into());

    // Compile and execute XPath using uiauto-xpath library
    let compile_start = Instant::now();
    let compiled_xpath = match XPath::compile(xpath) {
        Ok(xp) => xp,
        Err(e) => {
            error!("[XPath Validation] XPath compilation failed: {}", e);
            return Err(anyhow::anyhow!("XPath compilation error: {}", e));
        }
    };
    let compile_ms = compile_start.elapsed().as_millis();

    // Execute the query with optional visibility filter
    let execute_start = Instant::now();
    let matches: Vec<UIElement> = if strict {
        // Strict ControlView mode: no RawViewWalker fallback
        match compiled_xpath.select_nodes_strict(&uia_elem) {
            Ok(nodes) => {
                nodes.into_iter()
                    .map(|n| UIElement::from(n.raw_element_clone()))
                    .collect()
            },
            Err(e) => {
                error!("[XPath Validation] XPath strict execution failed: {}", e);
                return Err(anyhow::anyhow!("XPath execution error: {}", e));
            }
        }
    } else {
        match visibility_filter {
            Some(filter) => {
                match compiled_xpath.select_nodes_with_visibility(&uia_elem, filter) {
                    Ok(nodes) => {
                        nodes.into_iter()
                            .map(|n| UIElement::from(n.raw_element_clone()))
                            .collect()
                    },
                    Err(e) => {
                        error!("[XPath Validation] XPath execution with visibility filter failed: {}", e);
                        return Err(anyhow::anyhow!("XPath execution error: {}", e));
                    }
                }
            },
            None => {
                match compiled_xpath.select_nodes(&uia_elem) {
                    Ok(nodes) => {
                        nodes.into_iter()
                            .map(|n| UIElement::from(n.raw_element_clone()))
                            .collect()
                    },
                    Err(e) => {
                        error!("[XPath Validation] XPath execution failed: {}", e);
                        return Err(anyhow::anyhow!("XPath execution error: {}", e));
                    }
                }
            }
        }
    };

    let execute_ms = execute_start.elapsed().as_millis();
    let total_duration_ms = total_start.elapsed().as_millis() as u64;
    info!("[XPath Validation] Found {} matches ({}ms total)", matches.len(), total_duration_ms);
    info!(
        "[PERF][XPATH] detailed compile_ms={} execute_ms={} total_ms={} matches={} xpath_len={} descendant={}",
        compile_ms,
        execute_ms,
        total_duration_ms,
        matches.len(),
        xpath.len(),
        xpath.starts_with("//") || xpath.contains("//")
    );

    // Generate per-segment validation results for UI display.
    // Since uiauto-xpath executes the entire XPath at once, we split by `/`
    // to produce per-segment granularity consistent with Strategy 2.5/2.7 results.
    let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
    let segment_results: Vec<SegmentValidationResult> = if parts.is_empty() {
        vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: xpath.to_string(),
            matched: !matches.is_empty(),
            match_count: matches.len(),
            duration_ms: total_duration_ms,
            predicate_failures: Vec::new(),
        }]
    } else {
        parts.iter().enumerate().map(|(i, step)| {
            SegmentValidationResult {
                segment_index: i,
                segment_text: step.to_string(),
                matched: i < parts.len() - 1 || !matches.is_empty(),
                match_count: if i == parts.len() - 1 { matches.len() } else { 0 },
                duration_ms: if i == parts.len() - 1 { total_duration_ms } else { 0 },
                predicate_failures: Vec::new(),
            }
        }).collect()
    };

    Ok((matches, segment_results))
}

pub fn find_all_elements_detailed(
    window_selector: &str,
    element_xpath: &str,
    random_range: f32,
) -> Vec<crate::core::model::ElementData> {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };
    let windows = find_window_by_selector(&auto, window_selector);
    
    if windows.is_empty() {
        return vec![];
    }

    // ── Strip capture mode prefix ──
    let (capture_mode, element_xpath_stripped) = CaptureMode::strip_xpath_prefix(element_xpath);
    let is_child_mode = capture_mode.map_or(false, |m| m.is_child_mode());

    // ── Fast path: XPath directly targets Window elements ──
    // When the XPath is //Window[...] or /Window[...], we already have the
    // matching windows from find_window_by_selector. Instead of running the
    // expensive fallback chain on each window, apply the XPath predicates
    // directly to filter the already-enumerated windows.
    // Not applicable for child mode (child mode searches inside child HWNDs).
    if !is_child_mode {
        let xpath_trimmed = element_xpath_stripped.trim_start_matches('/');
        let targets_window = xpath_trimmed == "Window"
            || xpath_trimmed.starts_with("Window[");
        if targets_window {
            log::info!("[Fast Path] XPath targets Window, filtering enumerated windows directly");
            // Parse XPath predicates to filter windows
            let step_parsed = if xpath_trimmed.starts_with("Window[") {
                // Extract predicates from "Window[@Name='...' and ...]"
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
                // If there are predicates, filter
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
                    // Record Window fast path in cache
                    let _ = cache_store(element_xpath, window, CompiledStrategy::WindowFastPath, 0);
                    return vec![info];
                }
            }
            return vec![];
        }
    }
    // ── End fast path ──

    // ═══════════════════════════════════════════════════════════════════════════
    // Child Mode ([fast-child] / [full-child]): target is in child HWND.
    // Instead of searching from main window root, enumerate child HWNDs and
    // search directly from each child's UIA root element.
    // This skips the entire main window UI tree (hundreds of irrelevant nodes).
    // ═══════════════════════════════════════════════════════════════════════════
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

            // Get window rect for coordinate computation
            let window_rect = window.get_bounding_rectangle().ok().map(|r| {
                crate::core::model::Rect {
                    x: r.get_left(),
                    y: r.get_top(),
                    width: r.get_right() - r.get_left(),
                    height: r.get_bottom() - r.get_top(),
                }
            });

            for child_hwnd in &child_hwnds {
                let child_elem = match auto.element_from_handle((*child_hwnd).into()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                // Search XPath starting from this child window's UIA root
                let (elements, _) = match find_by_xpath_with_fallback(&auto, &child_elem, element_xpath) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if !elements.is_empty() {
                    let mut rng = rand::thread_rng();
                    return elements.iter().filter_map(|elem| {
                        element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
                    }).collect();
                }
            }
        }
        return vec![];
    }
    // ── End child mode ──

    // Try each matching window until we find elements.
    // When window_selector is broad (no filter predicates), we may get many windows.
    // Fallback chain is expensive (~2-6s per window for WebView XPaths), so:
    // 1. Cap at 3 windows max for broad selectors (covers 99% of cases)
    // 2. Skip WebView-class windows for /XPath (absolute path) — WebView containers
    //    are under child HWNDs, not direct children of the window.
    let is_broad_selector = !window_selector.contains('[');  // No predicates = matches everything
    let is_absolute_xpath = element_xpath_stripped.starts_with('/') && !element_xpath_stripped.starts_with("//");
    let max_windows = if is_broad_selector { 3usize } else { windows.len() };
    
    for (win_idx, window) in windows.iter().enumerate() {
        if win_idx >= max_windows {
            log::info!("[Find All] Reached max windows limit ({}/{}) for broad selector", win_idx, windows.len());
            break;
        }
        
        // Skip WebView-class windows for absolute XPath — their children live under child HWNDs
        if is_absolute_xpath {
            let win_class = window.get_classname().unwrap_or_default();
            if is_webview_class(&win_class) {
                log::info!("[Find All] Skipping WebView window '{}' for absolute XPath (children under child HWNDs)", win_class);
                continue;
            }
        }
        
        // 获取窗口矩形用于计算 visibleRect
        let window_rect = window.get_bounding_rectangle().ok().map(|r| {
            crate::core::model::Rect {
                x: r.get_left(),
                y: r.get_top(),
                width: r.get_right() - r.get_left(),
                height: r.get_bottom() - r.get_top(),
            }
        });

        let (elements, _) = match find_by_xpath_with_fallback(&auto, window, element_xpath) {
            Ok(result) => result,
            Err(_) => continue,
        };
        
        if !elements.is_empty() {
            let mut rng = rand::thread_rng();
            return elements.iter().filter_map(|elem| {
                element_info_from_uia(elem, window_rect.as_ref(), random_range, &mut rng)
            }).collect();
        }
    }
    
    vec![]
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

    // 获取 Desktop 矩形用于计算 visibleRect（Desktop 通常覆盖整个屏幕）
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

    // Get the base element's rect for visibleRect computation
    let base_rect = base_elem.get_bounding_rectangle().ok().map(|r| {
        crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });

    // Execute XPath search from the base element
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

/// Find elements by XPath from a cached parent element (migrated from com_worker).
///
/// Uses the element cache to look up a previously found parent element,
/// then searches within its subtree using XPath.
pub fn find_from_element_cached(
    runtime_id: &str,
    xpath: &str,
    random_range: f32,
) -> Vec<crate::core::model::ElementData> {
    use crate::core::element_cache::{cache_element, get_cached_element};

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

    let (results, raw_elements) = find_from_element_impl(&auto, &base_elem, xpath, random_range);

    // Cache found elements for subsequent lookups (single XPath execution)
    for raw_elem in &raw_elements {
        if let Some(rid_str) = runtime_id_key(raw_elem).map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")) {
            cache_element(rid_str, raw_elem.clone());
        }
    }

    results
}

