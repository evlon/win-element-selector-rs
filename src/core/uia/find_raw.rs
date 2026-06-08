// src/core/uia/find_raw.rs
//
// RawViewWalker based XPath search functions.
// Used by [full] / [full-child] mode — RawView only, never ControlView.
//
// Key functions:
// - search_descendants_via_raw_view: //XPath via RawView (FindAll + Chain)
// - search_descendants_depth_limited: with configurable max_depth
// - walk_raw_tree_steps: manual RawViewWalker tree walk
//
// CORE PRINCIPLE: [full] uses RawView ONLY, never ControlView!

use super::*;
use super::find::{
    build_uia_condition_from_step,
    element_matches_parsed_step,
    filter_findall_results,
    parse_xpath_step,
    step_has_complex_predicates,
};
use super::find_control::{
    search_descendants_via_uiauto_xpath,
    search_descendants_chain_find_first,
    search_descendants_chain_find_all,
};

// ═══════════════════════════════════════════════════════════════════════════
// RawView Descendant XPath Search
// ═══════════════════════════════════════════════════════════════════════════

/// RawView FindAll search for Full mode descendant XPaths.
///
/// Uses native UIA `FindAll(Subtree)` instead of manual BFS to find elements
/// matching the first XPath step. Complex predicates (starts-with/contains/matches)
/// are handled via secondary Rust-side filtering. Falls back to manual BFS only
/// when FindAll fails or cannot build conditions.
pub(super) fn search_descendants_via_raw_view(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    search_descendants_depth_limited(auto, window, xpath, 8, search_mode, filter)
}

pub(super) fn search_descendants_depth_limited(
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
    log::info!("[Raw Desc] First step: type={:?}, exact={:?}, complex={}",
        first_step_parsed.type_name, first_step_parsed.required_props, has_complex);

    // ═══════════════════════════════════════════════════════════════════════════
    // ★ Multi-step: Try Chain FindFirst/FindAll FIRST (fast path)
    //
    // Chain approach is O(N * single_step) where N = number of steps,
    // while FindAll(Subtree) can be O(total_tree_size) for the first step alone.
    //
    // Default: Chain only (fast, sufficient for locating single element).
    // When filter.enable_findall=true: Chain → FindAll(Subtree) fallback if Chain fails.
    // ═══════════════════════════════════════════════════════════════════════════
    if xpath_parts.len() > 1 {
        let chain_result = if search_mode != SearchMode::All {
            search_descendants_chain_find_first(auto, window, &xpath_parts, filter)
        } else {
            search_descendants_chain_find_all(auto, window, &xpath_parts, filter)
        };
        if let Some(results) = chain_result {
            if !results.is_empty() {
                let duration_ms = start.elapsed().as_millis() as u64;
                log::info!("[Raw Desc] ✓ Chain FindFirst/FindAll found {} results ({}ms) — fast path!",
                    results.len(), duration_ms);
                let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
                    SegmentValidationResult::matched(
                        i, step.to_string(),
                        if i == xpath_parts.len() - 1 { results.len() } else { 0 },
                        0,
                    )
                }).collect();
                return Ok((results, segments));
            }
            // Chain found nothing but was valid — fallback to TreeWalker (uiauto-xpath)
            // Reason: Chromium UIA virtualization means FindFirst(Subtree) cannot see
            // deep nodes, but RawViewWalker traversal triggers subtree expansion.
            // enable_findall only controls FindAll(Subtree), NOT TreeWalker fallback.
            log::info!("[Raw Desc] Chain found 0 results → falling back to TreeWalker (Chromium virtualization workaround)");
            let duration_ms = start.elapsed().as_millis() as u64;
            match search_descendants_via_uiauto_xpath(auto, window, xpath, None) {
                Ok((matches, segments)) if !matches.is_empty() => {
                    log::info!("[Raw Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), duration_ms);
                    return Ok((matches, segments));
                }
                Ok((_, segments)) => {
                    log::info!("[Raw Desc] TreeWalker fallback also found 0 results ({}ms total)", duration_ms);
                    return Ok((vec![], segments));
                }
                Err(e) => {
                    log::warn!("[Raw Desc] TreeWalker fallback failed: {}, returning not_found", e);
                    return Ok((vec![], vec![SegmentValidationResult::not_found(
                        0, xpath.to_string(), "RawTree", "Chain found 0 and TreeWalker fallback failed", duration_ms,
                    )]));
                }
            }
        } else {
            // Chain not applicable (complex predicates?) — fallback to TreeWalker
            log::info!("[Raw Desc] Chain not applicable → falling back to TreeWalker (Chromium virtualization workaround)");
            let duration_ms = start.elapsed().as_millis() as u64;
            match search_descendants_via_uiauto_xpath(auto, window, xpath, None) {
                Ok((matches, segments)) if !matches.is_empty() => {
                    log::info!("[Raw Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), duration_ms);
                    return Ok((matches, segments));
                }
                Ok((_, segments)) => {
                    log::info!("[Raw Desc] TreeWalker fallback also found 0 results ({}ms total)", duration_ms);
                    return Ok((vec![], segments));
                }
                Err(e) => {
                    log::warn!("[Raw Desc] TreeWalker fallback failed: {}, returning not_found", e);
                    return Ok((vec![], vec![SegmentValidationResult::not_found(
                        0, xpath.to_string(), "RawTree", "Chain not applicable and TreeWalker fallback failed", duration_ms,
                    )]));
                }
            }
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
                    log::info!("[Raw Desc] FindFirst(Subtree) found match in {}ms", t_find.elapsed().as_millis());
                    vec![element]
                }
                Err(e) => {
                    log::info!("[Raw Desc] FindFirst(Subtree) not found ({}ms): {:?}", t_find.elapsed().as_millis(), e);
                    Vec::new()
                }
            }
        } else {
            match window.find_all(TreeScope::Subtree, &cond) {
                Ok(candidates) => {
                    let raw_count = candidates.len();
                    log::info!("[Raw Desc] FindAll(Subtree) returned {} candidates in {}ms",
                        raw_count, t_find.elapsed().as_millis());

                    if has_complex {
                        let filtered: Vec<UIElement> = candidates
                            .into_iter()
                            .filter(|c| element_matches_parsed_step(c, &first_step_parsed))
                            .collect();
                        log::info!("[Raw Desc] After complex filter: {} → {} matches in {}ms",
                            raw_count, filtered.len(), t_find.elapsed().as_millis());
                        filter_findall_results(window, filtered, "RawDesc", filter)
                    } else {
                        filter_findall_results(window, candidates, "RawDesc", filter)
                    }
                }
                Err(e) => {
                    log::warn!("[Raw Desc] FindAll(Subtree) failed: {:?}, falling back to manual walk", e);
                    return search_descendants_via_raw_view_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
                }
            }
        }
    } else {
        log::info!("[Raw Desc] No UIA condition to build — using full manual walk");
        return search_descendants_via_raw_view_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
    };

    if first_step_matches.is_empty() {
        // FindAll/FindFirst returned 0 — fallback to TreeWalker (Chromium virtualization workaround)
        // Chromium UIA virtualizes deep nodes; FindAll(Subtree) can't see them,
        // but RawViewWalker traversal triggers subtree expansion.
        let duration_ms = start.elapsed().as_millis() as u64;
        log::info!("[Raw Desc] FindAll/FindFirst returned 0 for '{}' → falling back to TreeWalker ({}ms)", first_step, duration_ms);
        match search_descendants_via_uiauto_xpath(auto, window, xpath, None) {
            Ok((matches, segments)) if !matches.is_empty() => {
                log::info!("[Raw Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), start.elapsed().as_millis());
                return Ok((matches, segments));
            }
            Ok((_, segments)) => {
                log::info!("[Raw Desc] TreeWalker fallback also found 0 results ({}ms total)", start.elapsed().as_millis());
                return Ok((vec![], segments));
            }
            Err(e) => {
                log::warn!("[Raw Desc] TreeWalker fallback failed: {}", e);
                return Ok((vec![], vec![SegmentValidationResult::not_found(
                    0, first_step.to_string(), "RawTree", "FindAll returned 0 and TreeWalker fallback failed", duration_ms,
                )]));
            }
        }
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
        return Ok((first_step_matches, vec![SegmentValidationResult::matched(
            0, xpath.to_string(), match_count, duration_ms,
        )]));
    }

    // Strategy A: Try uiauto-xpath from each first-step match for the remaining XPath
    let remaining_has_descendant = remaining_xpath.contains("//");
    if remaining_has_descendant {
        log::info!("[Raw Desc] Skipping Strategy A (uiauto-xpath): remaining XPath has // descendant axis, using raw walk instead ({}ms)",
            start.elapsed().as_millis());
    } else {
        let t_strat_a = Instant::now();
        for (cand_idx, candidate) in first_step_matches.iter().enumerate() {
            let t_cand = Instant::now();
            if let Ok((matches, segments)) = search_descendants_via_uiauto_xpath(auto, candidate, &remaining_xpath, None) {
                log::info!("[Raw Desc] Strategy A candidate[{}]: uiauto-xpath took {}ms, {} matches",
                    cand_idx, t_cand.elapsed().as_millis(), matches.len());
                if !matches.is_empty() {
                    log::info!("[Raw Desc] ✓ Strategy A found {} from raw candidate[{}] ({}ms total)",
                        matches.len(), cand_idx, start.elapsed().as_millis());
                    let mut all_segments = vec![SegmentValidationResult::matched(
                        0, first_step.to_string(), 1, 0,
                    )];
                    for mut s in segments {
                        s.segment_index += 1;
                        all_segments.push(s);
                    }
                    return Ok((matches, all_segments));
                }
            }
        }
        log::info!("[Raw Desc] Strategy A failed from all {} candidates ({}ms total), falling back to raw tree walk",
            first_step_matches.len(), t_strat_a.elapsed().as_millis());
    }

    // Strategy B: Walk the raw tree manually for ALL remaining steps
    let raw_walker = match auto.get_raw_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Raw Desc] Failed to get RawViewWalker for fallback: {}", e);
            let duration_ms = start.elapsed().as_millis() as u64;
            return Ok((vec![], vec![SegmentValidationResult::not_found(
                0, first_step.to_string(), "RawTree", "RawViewWalker unavailable", duration_ms,
            )]));
        }
    };
    let t_strat_b = Instant::now();
    let mut all_matches = Vec::new();
    for candidate in &first_step_matches {
        if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, candidate, remaining_parts) {
            if !matches.is_empty() {
                all_matches.extend(matches);
            }
        }
    }

    log::info!("[Raw Desc] Strategy B raw walk: {}ms", t_strat_b.elapsed().as_millis());
    let duration_ms = start.elapsed().as_millis() as u64;
    log::info!("[Raw Desc] Full raw walk found {} matches ({}ms total)", all_matches.len(), duration_ms);

    let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
        SegmentValidationResult::matched(
            i, step.to_string(), if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 }, 0,
        )
    }).collect();

    Ok((all_matches, segments))
}

/// Manual BFS fallback for raw descendants when FindAll fails or can't build conditions.
fn search_descendants_via_raw_view_manual(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    xpath_parts: &[&str],
    first_step_parsed: &ParsedXPathStep,
    first_step: &str,
    start: &std::time::Instant,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    let max_depth: u32 = 8;
    let raw_walker = match auto.get_raw_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Raw Desc Manual] Failed to get RawViewWalker: {}", e);
            return Ok((vec![], vec![]));
        }
    };

    let mut queue: std::collections::VecDeque<(UIElement, u32)> = std::collections::VecDeque::from(vec![(window.clone(), 0)]);
    let mut bfs_nodes_visited = 0u64;
    let mut first_step_matches: Vec<UIElement> = Vec::new();

    while let Some((elem, depth)) = queue.pop_front() {
        let mut child = raw_walker.get_first_child(&elem).ok();
        while let Some(c) = child {
            bfs_nodes_visited += 1;
            if element_matches_parsed_step(&c, first_step_parsed) {
                first_step_matches.push(c.clone());
            }
            if depth + 1 < max_depth {
                queue.push_back((c.clone(), depth + 1));
            }
            child = raw_walker.get_next_sibling(&c).ok();
        }
    }

    log::info!("[Raw Desc Manual] BFS found {} first-step matches, visited {} nodes (depth limit={})",
        first_step_matches.len(), bfs_nodes_visited, max_depth);

    if first_step_matches.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        return Ok((vec![], vec![SegmentValidationResult::not_found(
            0, first_step.to_string(), "RawTree", "No raw tree element matches this step (manual fallback)", duration_ms,
        )]));
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
        return Ok((first_step_matches, vec![SegmentValidationResult::matched(
            0, xpath.to_string(), match_count, duration_ms,
        )]));
    }

    let remaining_has_descendant = remaining_xpath.contains("//");
    if !remaining_has_descendant {
        for candidate in &first_step_matches {
            if let Ok((matches, segments)) = search_descendants_via_uiauto_xpath(auto, candidate, &remaining_xpath, None) {
                if !matches.is_empty() {
                    let mut all_segments = vec![SegmentValidationResult::matched(
                        0, first_step.to_string(), 1, 0,
                    )];
                    for mut s in segments {
                        s.segment_index += 1;
                        all_segments.push(s);
                    }
                    return Ok((matches, all_segments));
                }
            }
        }
    }

    let mut all_matches = Vec::new();
    for candidate in &first_step_matches {
        if let Ok(matches) = walk_raw_tree_steps(auto, &raw_walker, candidate, remaining_parts) {
            if !matches.is_empty() {
                all_matches.extend(matches);
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    log::info!("[Raw Desc Manual] Full walk found {} matches ({}ms total)", all_matches.len(), duration_ms);

    let segments: Vec<SegmentValidationResult> = xpath_parts.iter().enumerate().map(|(i, step)| {
        SegmentValidationResult::matched(
            i, step.to_string(), if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 }, 0,
        )
    }).collect();

    Ok((all_matches, segments))
}

/// Walk the raw tree manually step by step (RawViewWalker).
pub(super) fn walk_raw_tree_steps(
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
