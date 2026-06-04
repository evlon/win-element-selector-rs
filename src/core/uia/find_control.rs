// src/core/uia/find_control.rs
//
// ControlViewWalker / uiauto-xpath based XPath search functions.
// Used by [fast] / [fast-child] mode — strict ControlView only, no RawView fallback.
//
// Key functions:
// - find_by_xpath_control_descendants: //XPath via ControlView (FindAll + Chain)
// - find_by_xpath_detailed: uiauto-xpath engine (ControlViewWalker based)
// - find_by_xpath_detailed_strict: uiauto-xpath strict mode (no RawView fallback)
// - walk_control_tree_steps: manual ControlViewWalker tree walk
// - findall_chain_first / findall_chain_all: Chain FindFirst/FindAll fast path

use super::*;
use super::find::{
    build_uia_condition_from_step,
    element_matches_parsed_step,
    filter_findall_results,
    parse_xpath_step,
    step_has_complex_predicates,
};

// ═══════════════════════════════════════════════════════════════════════════
// ControlView Descendant XPath Search
// ═══════════════════════════════════════════════════════════════════════════

/// ControlView FindAll search for Fast mode descendant XPaths.
///
/// Uses native UIA `FindAll(Subtree)` instead of manual BFS to find elements
/// matching the first XPath step. Complex predicates (starts-with/contains/matches)
/// are handled via secondary Rust-side filtering. Falls back to manual BFS only
/// when FindAll fails or cannot build conditions.
pub(super) fn find_by_xpath_control_descendants(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_control_descendants_with_depth(auto, window, xpath, 6, search_mode, filter)
}

fn find_by_xpath_control_descendants_with_depth(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    _max_depth: u32, // kept for API compatibility; FindAll doesn't need depth limit
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let start = Instant::now();

    // Parse XPath steps (skip leading // or /)
    let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
    if xpath_parts.is_empty() {
        return Ok((vec![], vec![]));
    }

    let first_step = xpath_parts[0];
    let first_step_parsed = parse_xpath_step(first_step);
    let has_complex = step_has_complex_predicates(&first_step_parsed);
    log::info!("[Ctrl Desc] First step: type={:?}, exact={:?}, complex={}",
        first_step_parsed.type_name, first_step_parsed.required_props, has_complex);

    // ═══════════════════════════════════════════════════════════════════════════
    // ★ Multi-step: Try Chain FindFirst/FindAll FIRST (fast path)
    //
    // Chain approach is O(N * single_step) where N = number of steps,
    // while FindAll(Subtree) can be O(total_tree_size) for the first step alone.
    // In Chrome WebView, FindAll(Group) takes 4735ms but Chain only needs 506ms.
    //
    // Default: Chain only (fast, sufficient for locating single element).
    // When filter.enable_findall=true: Chain → FindAll(Subtree) fallback if Chain fails.
    // ═══════════════════════════════════════════════════════════════════════════
    if xpath_parts.len() > 1 {
        let chain_result = if search_mode != SearchMode::All {
            findall_chain_first(auto, window, &xpath_parts, filter)
        } else {
            findall_chain_all(auto, window, &xpath_parts, filter)
        };
        if let Some(results) = chain_result {
            if !results.is_empty() {
                let duration_ms = start.elapsed().as_millis() as u64;
                log::info!("[Ctrl Desc] ✓ Chain FindFirst/FindAll found {} results ({}ms) — fast path!",
                    results.len(), duration_ms);
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
                return Ok((results, segments));
            }
            // Chain found nothing but was valid
            if !filter.enable_findall {
                // Default: skip expensive FindAll, return empty fast
                log::info!("[Ctrl Desc] Chain found 0 results, enable_findall=false → skipping FindAll (fast fail)");
                let duration_ms = start.elapsed().as_millis() as u64;
                return Ok((vec![], vec![SegmentValidationResult {
                    segment_index: 0,
                    segment_text: xpath.to_string(),
                    matched: false,
                    match_count: 0,
                    duration_ms,
                    predicate_failures: vec![super::PredicateFailure {
                        attr_name: "ControlTree".to_string(),
                        expected_value: xpath.to_string(),
                        actual_value: None,
                        reason: "Chain FindFirst found no matches (FindAll fallback disabled)".to_string(),
                    }],
                }]));
            }
            // enable_findall=true: Chain found 0 but was valid, fall through to FindAll
            log::info!("[Ctrl Desc] Chain found 0 results, enable_findall=true → trying FindAll fallback");
        } else {
            // Chain not applicable (complex predicates?)
            if !filter.enable_findall {
                log::info!("[Ctrl Desc] Chain not applicable, enable_findall=false → returning empty (fast fail)");
                let duration_ms = start.elapsed().as_millis() as u64;
                return Ok((vec![], vec![SegmentValidationResult {
                    segment_index: 0,
                    segment_text: xpath.to_string(),
                    matched: false,
                    match_count: 0,
                    duration_ms,
                    predicate_failures: vec![super::PredicateFailure {
                        attr_name: "ControlTree".to_string(),
                        expected_value: xpath.to_string(),
                        actual_value: None,
                        reason: "Chain not applicable (complex predicates), FindAll fallback disabled".to_string(),
                    }],
                }]));
            }
            log::info!("[Ctrl Desc] Chain not applicable (complex predicates?), enable_findall=true → falling back to FindAll");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Single-step or Chain fallback: Use native FindFirst/FindAll
    // ═══════════════════════════════════════════════════════════════════════════
    let t_find = Instant::now();
    let condition = build_uia_condition_from_step(auto, &first_step_parsed);

    let first_step_matches: Vec<UIElement> = if let Some(cond) = condition {
        let is_single_step = xpath_parts.len() == 1 && !has_complex;
        let use_find_first = is_single_step && search_mode != SearchMode::All;
        if use_find_first {
            match window.find_first(TreeScope::Subtree, &cond) {
                Ok(element) => {
                    log::info!("[Ctrl Desc] FindFirst(Subtree) found match in {}ms", t_find.elapsed().as_millis());
                    vec![element]
                }
                Err(e) => {
                    log::info!("[Ctrl Desc] FindFirst(Subtree) not found ({}ms): {:?}", t_find.elapsed().as_millis(), e);
                    Vec::new()
                }
            }
        } else {
            match window.find_all(TreeScope::Subtree, &cond) {
                Ok(candidates) => {
                    let raw_count = candidates.len();
                    log::info!("[Ctrl Desc] FindAll(Subtree) returned {} candidates in {}ms",
                        raw_count, t_find.elapsed().as_millis());

                    if has_complex {
                        let filtered: Vec<UIElement> = candidates
                            .into_iter()
                            .filter(|c| element_matches_parsed_step(c, &first_step_parsed))
                            .collect();
                        log::info!("[Ctrl Desc] After complex filter: {} → {} matches in {}ms",
                            raw_count, filtered.len(), t_find.elapsed().as_millis());
                        filter_findall_results(window, filtered, "CtrlDesc", filter)
                    } else {
                        filter_findall_results(window, candidates, "CtrlDesc", filter)
                    }
                }
                Err(e) => {
                    log::warn!("[Ctrl Desc] FindAll(Subtree) failed: {:?}, falling back to manual walk", e);
                    return find_by_xpath_control_descendants_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
                }
            }
        }
    } else {
        log::info!("[Ctrl Desc] No UIA condition to build — using full manual walk");
        return find_by_xpath_control_descendants_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
    };

    if first_step_matches.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        return Ok((vec![], vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: first_step.to_string(),
            matched: false,
            match_count: 0,
            duration_ms,
            predicate_failures: vec![super::PredicateFailure {
                attr_name: "ControlTree".to_string(),
                expected_value: first_step.to_string(),
                actual_value: None,
                reason: "No control tree element matches this step".to_string(),
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
        log::info!("[Ctrl Desc] First step is the last step, returning {} matches ({}ms)",
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

    // Strategy: Use uiauto-xpath (ControlView) from each first-step match
    let t_strat = Instant::now();
    for (cand_idx, candidate) in first_step_matches.iter().enumerate() {
        let t_cand = Instant::now();
        if let Ok((matches, segments)) = find_by_xpath_detailed_strict(auto, candidate, &remaining_xpath) {
            log::info!("[Ctrl Desc] candidate[{}]: strict uiauto-xpath took {}ms, {} matches",
                cand_idx, t_cand.elapsed().as_millis(), matches.len());
            if !matches.is_empty() {
                log::info!("[Ctrl Desc] ✓ Found {} from control candidate[{}] ({}ms total)",
                    matches.len(), cand_idx, start.elapsed().as_millis());
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
    log::info!("[Ctrl Desc] uiauto-xpath from {} candidates failed ({}ms total)",
        first_step_matches.len(), t_strat.elapsed().as_millis());

    // Fallback: walk remaining steps manually using ControlViewWalker
    let ctrl_walker = match auto.get_control_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Ctrl Desc] Failed to get ControlViewWalker for fallback: {}", e);
            let duration_ms = start.elapsed().as_millis() as u64;
            return Ok((vec![], vec![SegmentValidationResult {
                segment_index: 0,
                segment_text: first_step.to_string(),
                matched: false,
                match_count: 0,
                duration_ms,
                predicate_failures: vec![super::PredicateFailure {
                    attr_name: "ControlTree".to_string(),
                    expected_value: first_step.to_string(),
                    actual_value: None,
                    reason: "ControlViewWalker unavailable".to_string(),
                }],
            }]));
        }
    };
    let t_walk = Instant::now();
    let mut all_matches = Vec::new();
    for candidate in &first_step_matches {
        if let Ok(matches) = walk_control_tree_steps(auto, &ctrl_walker, candidate, remaining_parts) {
            if !matches.is_empty() {
                all_matches.extend(matches);
            }
        }
    }

    log::info!("[Ctrl Desc] Manual walk: {}ms, {} matches", t_walk.elapsed().as_millis(), all_matches.len());
    let duration_ms = start.elapsed().as_millis() as u64;
    log::info!("[Ctrl Desc] Full control walk found {} matches ({}ms total)", all_matches.len(), duration_ms);

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

/// Manual BFS fallback for when FindAll fails or can't build conditions.
/// Preserves the original ControlViewWalker BFS logic as a safety net.
fn find_by_xpath_control_descendants_manual(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    xpath_parts: &[&str],
    first_step_parsed: &ParsedXPathStep,
    first_step: &str,
    start: &std::time::Instant,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let max_depth: u32 = 6;
    let ctrl_walker = match auto.get_control_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Ctrl Desc Manual] Failed to get ControlViewWalker: {}", e);
            return Ok((vec![], vec![]));
        }
    };

    let mut queue: std::collections::VecDeque<(UIElement, u32)> = std::collections::VecDeque::from(vec![(window.clone(), 0)]);
    let mut bfs_nodes_visited = 0u64;
    let mut first_step_matches: Vec<UIElement> = Vec::new();

    while let Some((elem, depth)) = queue.pop_front() {
        let mut child = ctrl_walker.get_first_child(&elem).ok();
        while let Some(c) = child {
            bfs_nodes_visited += 1;
            if element_matches_parsed_step(&c, first_step_parsed) {
                first_step_matches.push(c.clone());
            }
            if depth + 1 < max_depth {
                queue.push_back((c.clone(), depth + 1));
            }
            child = ctrl_walker.get_next_sibling(&c).ok();
        }
    }

    log::info!("[Ctrl Desc Manual] BFS found {} first-step matches, visited {} nodes (depth limit={})",
        first_step_matches.len(), bfs_nodes_visited, max_depth);

    if first_step_matches.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        return Ok((vec![], vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: first_step.to_string(),
            matched: false,
            match_count: 0,
            duration_ms,
            predicate_failures: vec![super::PredicateFailure {
                attr_name: "ControlTree".to_string(),
                expected_value: first_step.to_string(),
                actual_value: None,
                reason: "No control tree element matches this step (manual fallback)".to_string(),
            }],
        }]));
    }

    let remaining_parts = &xpath_parts[1..];
    let remaining_xpath = if remaining_parts.is_empty() {
        String::new()
    } else {
        format!("/{}", remaining_parts.join("/"))
    };

    if remaining_parts.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        let match_count = first_step_matches.len();
        return Ok((first_step_matches, vec![SegmentValidationResult {
            segment_index: 0,
            segment_text: xpath.to_string(),
            matched: true,
            match_count,
            duration_ms,
            predicate_failures: Vec::new(),
        }]));
    }

    // Try uiauto-xpath from each first-step match for remaining steps
    for candidate in &first_step_matches {
        if let Ok((matches, segments)) = find_by_xpath_detailed_strict(auto, candidate, &remaining_xpath) {
            if !matches.is_empty() {
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

    // Fallback: walk remaining steps manually
    let mut all_matches = Vec::new();
    for candidate in &first_step_matches {
        if let Ok(matches) = walk_control_tree_steps(auto, &ctrl_walker, candidate, remaining_parts) {
            if !matches.is_empty() {
                all_matches.extend(matches);
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    log::info!("[Ctrl Desc Manual] Full walk found {} matches ({}ms total)", all_matches.len(), duration_ms);

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

/// Walk the control tree manually step by step (ControlViewWalker).
fn walk_control_tree_steps(
    auto: &UIAutomation,
    ctrl_walker: &uiautomation::UITreeWalker,
    root: &UIElement,
    steps: &[&str],
) -> anyhow::Result<Vec<UIElement>> {
    if steps.is_empty() {
        return Ok(vec![root.clone()]);
    }

    let current_step = steps[0];
    let parsed = parse_xpath_step(current_step);

    // Collect direct children matching this step
    let mut current_matches: Vec<UIElement> = Vec::new();
    let mut child = ctrl_walker.get_first_child(root).ok();
    while let Some(c) = child {
        if element_matches_parsed_step(&c, &parsed) {
            current_matches.push(c.clone());
        }
        child = ctrl_walker.get_next_sibling(&c).ok();
    }

    if current_matches.is_empty() || steps.len() == 1 {
        return Ok(current_matches);
    }

    // Recurse for remaining steps
    let remaining = &steps[1..];
    let mut all_matches = Vec::new();
    for candidate in &current_matches {
        if let Ok(sub_matches) = walk_control_tree_steps(auto, ctrl_walker, candidate, remaining) {
            all_matches.extend(sub_matches);
        }
    }

    Ok(all_matches)
}

// ═══════════════════════════════════════════════════════════════════════════
// uiauto-xpath Engine (ControlViewWalker based)
// ═══════════════════════════════════════════════════════════════════════════

/// Execute XPath using uiauto-xpath library (ControlViewWalker based).
/// Used by [fast] mode absolute XPaths and as a fallback in other strategies.
pub(super) fn find_by_xpath_detailed(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
    visibility_filter: Option<uiauto_xpath::xpath::VisibilityFilter>,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    find_by_xpath_detailed_impl(auto, root, xpath, visibility_filter, false)
}

/// Execute XPath using uiauto-xpath strict mode (no RawViewWalker fallback).
/// Used by [fast] mode — ControlView only, fails fast if element not in control tree.
pub(super) fn find_by_xpath_detailed_strict(
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

    let uia_elem = UiaXPathElement::new(root.clone().into(), auto.clone().into());

    let compile_start = Instant::now();
    let compiled_xpath = match XPath::compile(xpath) {
        Ok(xp) => xp,
        Err(e) => {
            error!("[XPath Validation] XPath compilation failed: {}", e);
            return Err(anyhow::anyhow!("XPath compilation error: {}", e));
        }
    };
    let compile_ms = compile_start.elapsed().as_millis();

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

// ═══════════════════════════════════════════════════════════════════════════
// Chain FindFirst/FindAll: Multi-layer descendant XPath fast path
//
// For `//A[@x='1']//B[@y='2']//C[@z='3']`, instead of:
//   FindAll(Subtree, A) → get all A candidates → for each A, uiauto-xpath → slow!
//
// We do:
//   FindFirst(Subtree, A) → a → FindFirst(Subtree, B) from a → b → FindFirst(Subtree, C) from b
//
// Each FindFirst is a single COM call, much faster than BFS/uiauto-xpath on
// large Chrome/WebView subtrees.
// ═══════════════════════════════════════════════════════════════════════════

/// Chain FindFirst: resolve multi-layer descendant XPath step by step.
/// Returns `Some(results)` if all steps resolved successfully, `None` if any step
/// cannot build a UIA condition (e.g., complex predicates) or FindFirst fails.
///
/// For SearchMode::First: uses FindFirst at each step (fastest).
/// For SearchMode::All: uses FindAll at the last step.
pub(super) fn findall_chain_first(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],
    filter: &FindAllFilter,
) -> Option<Vec<UIElement>> {
    use std::time::Instant;
    let chain_start = Instant::now();

    let mut current_root = root.clone();
    let mut step_times = Vec::new();

    for (step_idx, step_str) in xpath_parts.iter().enumerate() {
        let is_last = step_idx == xpath_parts.len() - 1;
        let parsed = parse_xpath_step(step_str);

        // Can't use FindFirst with complex predicates — they need secondary filtering
        if step_has_complex_predicates(&parsed) {
            log::info!("[Chain FindFirst] Step {}: complex predicates, falling back ({}ms)",
                step_idx, chain_start.elapsed().as_millis());
            return None;
        }

        let condition = build_uia_condition_from_step(auto, &parsed)?;
        let t_step = Instant::now();

        if is_last {
            // Last step: use FindFirst for :first mode (only need 1 result)
            match current_root.find_first(TreeScope::Subtree, &condition) {
                Ok(elem) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst(Subtree) found in {}ms", step_idx, ms);

                    // Apply post-filter (offscreen/zero-size/out-of-bounds)
                    let results = filter_findall_results(root, vec![elem], "ChainFirst", filter);
                    if results.is_empty() {
                        log::info!("[Chain FindFirst] Step {}: filtered to 0 results", step_idx);
                        return Some(vec![]);
                    }

                    log::info!("[Chain FindFirst] ✓ All {} steps resolved in {}ms [{}]",
                        xpath_parts.len(), chain_start.elapsed().as_millis(),
                        step_times.iter().map(|ms| format!("{}ms", ms)).collect::<Vec<_>>().join(", "));
                    return Some(results);
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst(Subtree) not found ({}ms): {:?}", step_idx, ms, e);
                    // FindFirst returned "not found" — the Chain IS applicable (condition was built,
                    // no complex predicates), it just found 0 results. Return Some(vec![]) so the
                    // caller can distinguish "found 0" from "truly not applicable" (None).
                    return Some(vec![]);
                }
            }
        } else {
            // Intermediate step: use FindFirst to narrow down to single candidate
            match current_root.find_first(TreeScope::Subtree, &condition) {
                Ok(elem) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst(Subtree) found in {}ms", step_idx, ms);
                    current_root = elem;
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst(Subtree) not found ({}ms): {:?}", step_idx, ms, e);
                    // Intermediate step not found — can't continue Chain, return empty
                    return Some(vec![]);
                }
            }
        }
    }

    // Should not reach here, but just in case
    None
}

/// Chain FindAll: resolve multi-layer descendant XPath, collecting ALL matches at the last step.
/// Returns `Some(results)` if all steps resolved successfully, `None` if any step
/// cannot build a UIA condition or an intermediate FindFirst fails.
pub(super) fn findall_chain_all(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],
    filter: &FindAllFilter,
) -> Option<Vec<UIElement>> {
    use std::time::Instant;
    let chain_start = Instant::now();

    let mut current_root = root.clone();
    let mut step_times = Vec::new();

    for (step_idx, step_str) in xpath_parts.iter().enumerate() {
        let is_last = step_idx == xpath_parts.len() - 1;
        let parsed = parse_xpath_step(step_str);

        if step_has_complex_predicates(&parsed) {
            log::info!("[Chain FindAll] Step {}: complex predicates, falling back ({}ms)",
                step_idx, chain_start.elapsed().as_millis());
            return None;
        }

        let condition = build_uia_condition_from_step(auto, &parsed)?;
        let t_step = Instant::now();

        if is_last {
            // Last step: use FindAll to collect all matches
            match current_root.find_all(TreeScope::Subtree, &condition) {
                Ok(elems) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindAll(Subtree) found {} in {}ms", step_idx, elems.len(), ms);

                    let results = filter_findall_results(root, elems, "ChainAll", filter);
                    if results.is_empty() {
                        log::info!("[Chain FindAll] Step {}: filtered to 0 results", step_idx);
                        return Some(vec![]);
                    }

                    log::info!("[Chain FindAll] ✓ All {} steps resolved in {}ms [{}]",
                        xpath_parts.len(), chain_start.elapsed().as_millis(),
                        step_times.iter().map(|ms| format!("{}ms", ms)).collect::<Vec<_>>().join(", "));
                    return Some(results);
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindAll(Subtree) not found ({}ms): {:?}", step_idx, ms, e);
                    return Some(vec![]);
                }
            }
        } else {
            // Intermediate step: use FindFirst to narrow down
            match current_root.find_first(TreeScope::Subtree, &condition) {
                Ok(elem) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindFirst(Subtree) found in {}ms", step_idx, ms);
                    current_root = elem;
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindFirst(Subtree) not found ({}ms): {:?}", step_idx, ms, e);
                    return Some(vec![]);
                }
            }
        }
    }

    None
}
