// src/core/uia/find_control.rs
//
// ControlViewWalker / uiauto-xpath based XPath search functions.
// Used by [fast] / [fast-child] mode — strict ControlView only, no RawView fallback.
//
// Key functions:
// - search_descendants_via_control_view: //XPath via ControlView (FindAll + Chain)
// - search_descendants_via_uiauto_xpath: uiauto-xpath engine (ControlViewWalker based)
// - search_descendants_via_control_walker: uiauto-xpath strict mode (no RawView fallback)
// - walk_control_tree_steps: manual ControlViewWalker tree walk
// - search_descendants_chain_find_first / search_descendants_chain_find_all: Chain FindFirst/FindAll fast path

use super::*;
use super::find::{
    build_uia_condition_from_step,
    element_matches_parsed_step,
    filter_findall_results,
    parse_xpath_step,
    step_has_complex_predicates,
    split_xpath_steps,
    reconstruct_descendant_xpath,
    strip_step_prefix,
};
use super::cache::PositionPredicate;

/// Chain FindFirst 加速上限：position()=N 中 N 的最大允许值。
/// - position()=1: FindFirst (最快，O(1))
/// - position()=N (2 <= N <= MAX_N): FindAll + 取第N个
/// - position()=last() 或 N > MAX_N: 降级到 uiauto-xpath 引擎
/// 默认值 1 表示只加速 position()=1（即 FindFirst），需要更大值时手动调整。
const FINDFIRST_NEXT_MAX_N: i32 = 1;
use uiautomation::patterns::UIItemContainerPattern;

/// Generate a brief element summary for logging: "Type[ClassName='xx', Name='yy']"
fn elem_summary(elem: &UIElement) -> String {
    let ct = elem.get_control_type().map(|ct| format!("{:?}", ct)).unwrap_or_else(|_| "?".into());
    let cn = elem.get_classname().unwrap_or_default();
    let nm = elem.get_name().unwrap_or_default();
    if nm.is_empty() {
        format!("{}[@ClassName='{}']", ct, cn)
    } else {
        format!("{}[@ClassName='{}', @Name='{}']", ct, cn, nm)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ControlView Descendant XPath Search
// ═══════════════════════════════════════════════════════════════════════════

/// ControlView FindAll search for Fast mode descendant XPaths.
///
/// Uses native UIA `FindAll(Subtree)` instead of manual BFS to find elements
/// matching the first XPath step. Complex predicates (starts-with/contains/matches)
/// are handled via secondary Rust-side filtering. Falls back to manual BFS only
/// when FindAll fails or cannot build conditions.
pub(super) fn search_descendants_via_control_view(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    search_descendants_via_control_view_impl(auto, window, xpath, 6, search_mode, filter)
}

fn search_descendants_via_control_view_impl(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    _max_depth: u32, // kept for API compatibility; FindAll doesn't need depth limit
    search_mode: SearchMode,
    filter: &FindAllFilter,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    use std::time::Instant;
    let start = Instant::now();

    // Parse XPath steps — 保留前缀（//、/、/*n/），避免轴信息丢失
    let xpath_parts: Vec<&str> = split_xpath_steps(xpath);
    if xpath_parts.is_empty() {
        return Ok((vec![], vec![]));
    }

    let first_step = xpath_parts[0];
    // 去掉第一步的前缀来解析节点测试和谓词
    let first_step_body = strip_step_prefix(first_step);
    let first_step_parsed = parse_xpath_step(first_step_body);
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
            search_descendants_chain_find_first(auto, window, &xpath_parts, filter)
        } else {
            search_descendants_chain_find_all(auto, window, &xpath_parts, filter)
        };
        match chain_result {
            ChainResult::Complete(results) => {
                if !results.is_empty() {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    log::info!("[Ctrl Desc] ✓ Chain FindFirst/FindAll found {} results ({}ms) — fast path!",
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
                // Chain found nothing but was valid — try ItemContainerPattern, then TreeWalker
                // Reason: Chromium UIA virtualization means FindFirst(Subtree) cannot see
                // deep nodes, but ControlViewWalker traversal triggers subtree expansion.
                // enable_findall only controls FindAll(Subtree), NOT TreeWalker fallback.
                log::info!("[Ctrl Desc] Chain found 0 results → trying ItemContainerPattern then TreeWalker");

                // P0: Try ItemContainerPattern first (O(1) lookup if supported)
                if let Ok(Some(elem)) = try_item_container_search(window, xpath) {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    log::info!("[Ctrl Desc] ✓ ItemContainerPattern found result ({}ms)", duration_ms);
                    let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
                    let segments: Vec<SegmentValidationResult> = parts.iter().enumerate().map(|(i, step)| {
                        SegmentValidationResult::matched(i, step.to_string(), if i == parts.len() - 1 { 1 } else { 0 }, 0)
                    }).collect();
                    return Ok((vec![elem], segments));
                }

                // Fallback: TreeWalker (uiauto-xpath) from root
                let duration_ms = start.elapsed().as_millis() as u64;
                match search_descendants_via_control_walker(auto, window, xpath) {
                    Ok((matches, segments)) if !matches.is_empty() => {
                        log::info!("[Ctrl Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), duration_ms);
                        return Ok((matches, segments));
                    }
                    Ok((_, segments)) => {
                        log::info!("[Ctrl Desc] TreeWalker fallback also found 0 results ({}ms total)", duration_ms);
                        return Ok((vec![], segments));
                    }
                    Err(e) => {
                        log::warn!("[Ctrl Desc] TreeWalker fallback failed: {}, returning not_found", e);
                        return Ok((vec![], vec![SegmentValidationResult::not_found(
                            0, xpath.to_string(), "ControlTree", "Chain found 0 and TreeWalker fallback failed", duration_ms,
                        )]));
                    }
                }
            }
            ChainResult::Partial(progress) => {
                // P1: Chain partially succeeded — use narrowed scope for TreeWalker fallback
                // Instead of searching from root, search from the last successful element's subtree
                let remaining_parts = &xpath_parts[progress.last_successful_step + 1..];
                let remaining_xpath = reconstruct_descendant_xpath(remaining_parts);
                log::info!("[Ctrl Desc] Chain partial: steps 0..{} resolved, searching '{}' from {} ({}ms)",
                    progress.last_successful_step, remaining_xpath, elem_summary(&progress.last_element),
                    start.elapsed().as_millis());

                // Try ItemContainerPattern first (use remaining_xpath from partial element)
                if let Ok(Some(elem)) = try_item_container_search(&progress.last_element, &remaining_xpath) {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    log::info!("[Ctrl Desc] ✓ ItemContainerPattern found result ({}ms)", duration_ms);
                    let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
                    let segments: Vec<SegmentValidationResult> = parts.iter().enumerate().map(|(i, step)| {
                        SegmentValidationResult::matched(i, step.to_string(), if i == parts.len() - 1 { 1 } else { 0 }, 0)
                    }).collect();
                    return Ok((vec![elem], segments));
                }

                // TreeWalker from the partial element (narrowed scope — much faster than from root)
                match search_descendants_via_control_walker(auto, &progress.last_element, &remaining_xpath) {
                    Ok((matches, segments)) if !matches.is_empty() => {
                        let duration_ms = start.elapsed().as_millis() as u64;
                        log::info!("[Ctrl Desc] ✓ TreeWalker from partial element found {} results ({}ms total)", matches.len(), duration_ms);
                        // Build full segment results: mark earlier steps as matched
                        let mut all_segments: Vec<SegmentValidationResult> = xpath_parts[..=progress.last_successful_step]
                            .iter().enumerate().map(|(i, step)| {
                                SegmentValidationResult::matched(i, step.to_string(), 0, 0)
                            }).collect();
                        for mut s in segments {
                            s.segment_index += progress.last_successful_step + 1;
                            all_segments.push(s);
                        }
                        return Ok((matches, all_segments));
                    }
                    Ok((_, _segments)) => {
                        let duration_ms = start.elapsed().as_millis() as u64;
                        log::info!("[Ctrl Desc] TreeWalker from partial element also found 0 results ({}ms total)", duration_ms);
                        // Fallback: try from root as last resort
                        match search_descendants_via_control_walker(auto, window, xpath) {
                            Ok((matches, segs)) if !matches.is_empty() => {
                                log::info!("[Ctrl Desc] ✓ TreeWalker from root found {} results ({}ms)", matches.len(), start.elapsed().as_millis());
                                return Ok((matches, segs));
                            }
                            Ok((_, segs)) => return Ok((vec![], segs)),
                            Err(e) => {
                                log::warn!("[Ctrl Desc] TreeWalker from root also failed: {}", e);
                                return Ok((vec![], vec![SegmentValidationResult::not_found(
                                    0, xpath.to_string(), "ControlTree", "Partial Chain and TreeWalker fallback failed", duration_ms,
                                )]));
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[Ctrl Desc] TreeWalker from partial element failed: {}, trying from root", e);
                        match search_descendants_via_control_walker(auto, window, xpath) {
                            Ok((matches, segs)) if !matches.is_empty() => {
                                log::info!("[Ctrl Desc] ✓ TreeWalker from root found {} results ({}ms)", matches.len(), start.elapsed().as_millis());
                                return Ok((matches, segs));
                            }
                            Ok((_, segs)) => return Ok((vec![], segs)),
                            Err(e2) => {
                                log::warn!("[Ctrl Desc] TreeWalker from root also failed: {}", e2);
                                let duration_ms = start.elapsed().as_millis() as u64;
                                return Ok((vec![], vec![SegmentValidationResult::not_found(
                                    0, xpath.to_string(), "ControlTree", "Partial Chain and TreeWalker fallback failed", duration_ms,
                                )]));
                            }
                        }
                    }
                }
            }
            ChainResult::NotApplicable => {
                // Chain not applicable — try ItemContainerPattern, then TreeWalker
                log::info!("[Ctrl Desc] Chain not applicable → trying ItemContainerPattern then TreeWalker");

                // P0: Try ItemContainerPattern first
                if let Ok(Some(elem)) = try_item_container_search(window, xpath) {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    log::info!("[Ctrl Desc] ✓ ItemContainerPattern found result ({}ms)", duration_ms);
                    let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
                    let segments: Vec<SegmentValidationResult> = parts.iter().enumerate().map(|(i, step)| {
                        SegmentValidationResult::matched(i, step.to_string(), if i == parts.len() - 1 { 1 } else { 0 }, 0)
                    }).collect();
                    return Ok((vec![elem], segments));
                }

                // Fallback: TreeWalker
                let duration_ms = start.elapsed().as_millis() as u64;
                match search_descendants_via_control_walker(auto, window, xpath) {
                    Ok((matches, segments)) if !matches.is_empty() => {
                        log::info!("[Ctrl Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), duration_ms);
                        return Ok((matches, segments));
                    }
                    Ok((_, segments)) => {
                        log::info!("[Ctrl Desc] TreeWalker fallback also found 0 results ({}ms total)", duration_ms);
                        return Ok((vec![], segments));
                    }
                    Err(e) => {
                        log::warn!("[Ctrl Desc] TreeWalker fallback failed: {}, returning not_found", e);
                        return Ok((vec![], vec![SegmentValidationResult::not_found(
                            0, xpath.to_string(), "ControlTree", "Chain not applicable and TreeWalker fallback failed", duration_ms,
                        )]));
                    }
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
                    return search_descendants_via_control_view_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
                }
            }
        }
    } else {
        log::info!("[Ctrl Desc] No UIA condition to build — using full manual walk");
        return search_descendants_via_control_view_manual(auto, window, xpath, &xpath_parts, &first_step_parsed, first_step, &start);
    };

    if first_step_matches.is_empty() {
        // FindAll/FindFirst returned 0 — try ItemContainerPattern, then TreeWalker
        let duration_ms = start.elapsed().as_millis() as u64;
        log::info!("[Ctrl Desc] FindAll/FindFirst returned 0 for '{}' → trying ItemContainerPattern then TreeWalker ({}ms)", first_step, duration_ms);

        // P0: Try ItemContainerPattern first
        if let Ok(Some(elem)) = try_item_container_search(window, xpath) {
            log::info!("[Ctrl Desc] ✓ ItemContainerPattern found result ({}ms)", start.elapsed().as_millis());
            return Ok((vec![elem], vec![SegmentValidationResult::matched(
                0, first_step.to_string(), 1, start.elapsed().as_millis() as u64,
            )]));
        }

        // Fallback: TreeWalker
        match search_descendants_via_control_walker(auto, window, xpath) {
            Ok((matches, segments)) if !matches.is_empty() => {
                log::info!("[Ctrl Desc] ✓ TreeWalker fallback found {} results ({}ms total)", matches.len(), start.elapsed().as_millis());
                return Ok((matches, segments));
            }
            Ok((_, segments)) => {
                log::info!("[Ctrl Desc] TreeWalker fallback also found 0 results ({}ms total)", start.elapsed().as_millis());
                return Ok((vec![], segments));
            }
            Err(e) => {
                log::warn!("[Ctrl Desc] TreeWalker fallback failed: {}", e);
                return Ok((vec![], vec![SegmentValidationResult::not_found(
                    0, first_step.to_string(), "ControlTree", "FindAll returned 0 and TreeWalker fallback failed", duration_ms,
                )]));
            }
        }
    }

    // Build remaining XPath (steps after the first)
    let remaining_parts = &xpath_parts[1..];
    let remaining_xpath = if remaining_parts.is_empty() {
        String::new()
    } else {
        reconstruct_descendant_xpath(remaining_parts)
    };

    // If no remaining steps, the first-step matches ARE the result
    if remaining_parts.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        let match_count = first_step_matches.len();
        log::info!("[Ctrl Desc] First step is the last step, returning {} matches ({}ms)",
            match_count, duration_ms);
        return Ok((first_step_matches, vec![SegmentValidationResult::matched(
            0, xpath.to_string(), match_count, duration_ms,
        )]));
    }

    // Strategy: Use uiauto-xpath (ControlView) from each first-step match
    let t_strat = Instant::now();
    for (cand_idx, candidate) in first_step_matches.iter().enumerate() {
        let t_cand = Instant::now();
        if let Ok((matches, segments)) = search_descendants_via_control_walker(auto, candidate, &remaining_xpath) {
            log::info!("[Ctrl Desc] candidate[{}]: strict uiauto-xpath took {}ms, {} matches",
                cand_idx, t_cand.elapsed().as_millis(), matches.len());
            if !matches.is_empty() {
                log::info!("[Ctrl Desc] ✓ Found {} from control candidate[{}] ({}ms total)",
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
    log::info!("[Ctrl Desc] uiauto-xpath from {} candidates failed ({}ms total)",
        first_step_matches.len(), t_strat.elapsed().as_millis());

    // Fallback: walk remaining steps manually using ControlViewWalker
    let ctrl_walker = match auto.get_control_view_walker() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[Ctrl Desc] Failed to get ControlViewWalker for fallback: {}", e);
            let duration_ms = start.elapsed().as_millis() as u64;
            return Ok((vec![], vec![SegmentValidationResult::not_found(
                0, first_step.to_string(), "ControlTree", "ControlViewWalker unavailable", duration_ms,
            )]));
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
        SegmentValidationResult::matched(
            i, step.to_string(), if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 }, 0,
        )
    }).collect();

    Ok((all_matches, segments))
}

/// Manual BFS fallback for when FindAll fails or can't build conditions.
/// Preserves the original ControlViewWalker BFS logic as a safety net.
fn search_descendants_via_control_view_manual(
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
        return Ok((vec![], vec![SegmentValidationResult::not_found(
            0, first_step.to_string(), "ControlTree", "No control tree element matches this step (manual fallback)", duration_ms,
        )]));
    }

    let remaining_parts = &xpath_parts[1..];
    let remaining_xpath = if remaining_parts.is_empty() {
        String::new()
    } else {
        reconstruct_descendant_xpath(remaining_parts)
    };

    if remaining_parts.is_empty() {
        let duration_ms = start.elapsed().as_millis() as u64;
        let match_count = first_step_matches.len();
        return Ok((first_step_matches, vec![SegmentValidationResult::matched(
            0, xpath.to_string(), match_count, duration_ms,
        )]));
    }

    // Try uiauto-xpath from each first-step match for remaining steps
    for candidate in &first_step_matches {
        if let Ok((matches, segments)) = search_descendants_via_control_walker(auto, candidate, &remaining_xpath) {
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
        SegmentValidationResult::matched(
            i, step.to_string(), if i == xpath_parts.len() - 1 { all_matches.len() } else { 0 }, 0,
        )
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
pub(super) fn search_descendants_via_uiauto_xpath(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
    visibility_filter: Option<uiauto_xpath::xpath::VisibilityFilter>,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    search_uiauto_xpath_core(auto, root, xpath, visibility_filter, false)
}

/// Execute XPath using uiauto-xpath strict mode (no RawViewWalker fallback).
/// Used by [fast] mode — ControlView only, fails fast if element not in control tree.
pub(super) fn search_descendants_via_control_walker(
    auto: &UIAutomation,
    root: &UIElement,
    xpath: &str,
) -> anyhow::Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    search_uiauto_xpath_core(auto, root, xpath, None, true)
}

fn search_uiauto_xpath_core(
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
        vec![SegmentValidationResult::matched(0, xpath.to_string(), matches.len(), total_duration_ms)]
    } else {
        parts.iter().enumerate().map(|(i, step)| {
            SegmentValidationResult::matched(
                i, step.to_string(),
                if i == parts.len() - 1 { matches.len() } else { 0 },
                if i == parts.len() - 1 { total_duration_ms } else { 0 },
            )
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

/// Chain execution progress: which step succeeded last, and the element found.
/// Used for P1 optimization: when Chain partially succeeds, we can narrow
/// the TreeWalker search scope to the last successful element's subtree.
pub(super) struct ChainProgress {
    /// Index of the last successfully resolved step (0-based)
    pub last_successful_step: usize,
    /// The element found at that step (will be used as root for TreeWalker)
    pub last_element: UIElement,
}

/// Result of Chain execution: either fully resolved, partially resolved, or not applicable.
pub(super) enum ChainResult {
    /// All steps resolved successfully
    Complete(Vec<UIElement>),
    /// Some steps resolved, then failed. Contains progress info for fallback optimization.
    Partial(ChainProgress),
    /// Chain not applicable (e.g., complex predicates)
    NotApplicable,
}

/// Chain FindFirst: resolve multi-layer descendant XPath step by step.
/// Returns `ChainResult` indicating whether all steps resolved, partially resolved,
/// or Chain was not applicable (e.g., complex predicates).
///
/// For SearchMode::First: uses FindFirst at each step (fastest).
/// For SearchMode::All: uses FindAll at the last step.
pub(super) fn search_descendants_chain_find_first(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],
    filter: &FindAllFilter,
) -> ChainResult {
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
            if step_idx > 0 {
                return ChainResult::Partial(ChainProgress {
                    last_successful_step: step_idx - 1,
                    last_element: current_root,
                });
            }
            return ChainResult::NotApplicable;
        }

        // Check if position predicate is within FINDFIRST_NEXT_MAX_N limit
        // position()=None → treated as position()=1 (take first)
        // position()=1 → FindFirst (always OK)
        // position()=N (2<=N<=MAX_N) → FindAll + pick N-th (acceptable)
        // position()=last() or N > MAX_N → degrade to uiauto-xpath
        let position_n = match &parsed.position {
            None => 1,           // No position → default to first
            Some(PositionPredicate::Index(1)) => 1,
            Some(PositionPredicate::Index(n)) if *n >= 2 && *n <= FINDFIRST_NEXT_MAX_N => *n,
            Some(PositionPredicate::Index(n)) if *n > FINDFIRST_NEXT_MAX_N => {
                log::info!("[Chain FindFirst] Step {}: position()={} > FINDFIRST_NEXT_MAX_N={}, degrading ({}ms)",
                    step_idx, n, FINDFIRST_NEXT_MAX_N, chain_start.elapsed().as_millis());
                if step_idx > 0 {
                    return ChainResult::Partial(ChainProgress {
                        last_successful_step: step_idx - 1,
                        last_element: current_root,
                    });
                }
                return ChainResult::NotApplicable;
            }
            Some(PositionPredicate::Last) => {
                log::info!("[Chain FindFirst] Step {}: position()=last() not supported, degrading ({}ms)",
                    step_idx, chain_start.elapsed().as_millis());
                if step_idx > 0 {
                    return ChainResult::Partial(ChainProgress {
                        last_successful_step: step_idx - 1,
                        last_element: current_root,
                    });
                }
                return ChainResult::NotApplicable;
            }
            Some(PositionPredicate::Index(_)) => 1, // edge case (position()=0 etc) → FindFirst
        };

        let condition = match build_uia_condition_from_step(auto, &parsed) {
            Some(c) => c,
            None => {
                log::info!("[Chain FindFirst] Step {}: cannot build UIA condition ({}ms)",
                    step_idx, chain_start.elapsed().as_millis());
                if step_idx > 0 {
                    return ChainResult::Partial(ChainProgress {
                        last_successful_step: step_idx - 1,
                        last_element: current_root,
                    });
                }
                return ChainResult::NotApplicable;
            }
        };
        let t_step = Instant::now();

        let scope = match &parsed.prefix {
            XPathStepPrefix::Child => TreeScope::Children,
            XPathStepPrefix::Descendant | XPathStepPrefix::DepthLimited { .. } => TreeScope::Descendants,
        };
        let scope_name = match scope {
            TreeScope::Children => "Children",
            TreeScope::Descendants => "Descendants",
            _ => "Other",
        };

        // Resolve element based on position_n
        if position_n == 1 {
            // position()=1 or no position → FindFirst (fastest path)
            match current_root.find_first(scope, &condition) {
                Ok(elem) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst({}) found {} in {}ms", step_idx, scope_name, elem_summary(&elem), ms);
                    if is_last {
                        let results = filter_findall_results(root, vec![elem], "ChainFirst", filter);
                        if results.is_empty() {
                            log::info!("[Chain FindFirst] Step {}: filtered to 0 results", step_idx);
                            return ChainResult::Complete(vec![]);
                        }
                        log::info!("[Chain FindFirst] ✓ All {} steps resolved in {}ms [{}]",
                            xpath_parts.len(), chain_start.elapsed().as_millis(),
                            step_times.iter().map(|ms| format!("{}ms", ms)).collect::<Vec<_>>().join(", "));
                        return ChainResult::Complete(results);
                    }
                    current_root = elem;
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindFirst({}) not found '{}' ({}ms): {:?}", step_idx, scope_name, step_str, ms, e);
                    if is_last {
                        return ChainResult::Complete(vec![]);
                    }
                    if step_idx > 0 {
                        return ChainResult::Partial(ChainProgress {
                            last_successful_step: step_idx - 1,
                            last_element: current_root,
                        });
                    }
                    return ChainResult::Complete(vec![]);
                }
            }
        } else {
            // position()=N (2 <= N <= FINDFIRST_NEXT_MAX_N) → FindAll + pick N-th
            match current_root.find_all(scope, &condition) {
                Ok(elems) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    let idx = (position_n - 1) as usize;
                    if idx < elems.len() {
                        let elem = elems[idx].clone();
                        log::info!("[Chain FindFirst] Step {}: FindAll({}) found {} elems, picked [{}] {} in {}ms",
                            step_idx, scope_name, elems.len(), position_n, elem_summary(&elem), ms);
                        if is_last {
                            let results = filter_findall_results(root, vec![elem], "ChainFirst", filter);
                            if results.is_empty() {
                                log::info!("[Chain FindFirst] Step {}: filtered to 0 results", step_idx);
                                return ChainResult::Complete(vec![]);
                            }
                            log::info!("[Chain FindFirst] ✓ All {} steps resolved in {}ms [{}]",
                                xpath_parts.len(), chain_start.elapsed().as_millis(),
                                step_times.iter().map(|ms| format!("{}ms", ms)).collect::<Vec<_>>().join(", "));
                            return ChainResult::Complete(results);
                        }
                        current_root = elem;
                    } else {
                        log::info!("[Chain FindFirst] Step {}: FindAll({}) found {} elems, but need position {} ({}ms)",
                            step_idx, scope_name, elems.len(), position_n, ms);
                        if is_last {
                            return ChainResult::Complete(vec![]);
                        }
                        if step_idx > 0 {
                            return ChainResult::Partial(ChainProgress {
                                last_successful_step: step_idx - 1,
                                last_element: current_root,
                            });
                        }
                        return ChainResult::Complete(vec![]);
                    }
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindFirst] Step {}: FindAll({}) failed '{}' ({}ms): {:?}", step_idx, scope_name, step_str, ms, e);
                    if is_last {
                        return ChainResult::Complete(vec![]);
                    }
                    if step_idx > 0 {
                        return ChainResult::Partial(ChainProgress {
                            last_successful_step: step_idx - 1,
                            last_element: current_root,
                        });
                    }
                    return ChainResult::Complete(vec![]);
                }
            }
        }
    }

    // Should not reach here, but just in case
    ChainResult::NotApplicable
}

// ═══════════════════════════════════════════════════════════════════════════
// ItemContainerPattern optimization
//
// ItemContainerPattern::FindItemByProperty is a native UIA API designed for
// virtualized containers. It can directly search for a child by property
// (e.g., AutomationId) without traversing the entire tree, which is
// dramatically faster than TreeWalker when dealing with Chromium UIA
// virtualization.
//
// Strategy: Before falling back to the expensive TreeWalker traversal,
// check if the root element supports ItemContainerPattern. If it does,
// try FindItemByProperty for each simple predicate in the XPath.
// ═══════════════════════════════════════════════════════════════════════════

/// Try to find an element using ItemContainerPattern::FindItemByProperty.
///
/// Only supports simple property lookups (AutomationId, Name, ClassName).
/// Returns `Ok(Some(element))` if found, `Ok(None)` if pattern not available
/// or property not supported, `Err` on actual failure.
pub(super) fn try_item_container_search(
    root: &UIElement,
    xpath: &str,
) -> anyhow::Result<Option<UIElement>> {
    use std::time::Instant;
    let start = Instant::now();

    // Step 1: Check if root supports ItemContainerPattern
    let container_pattern: UIItemContainerPattern = match root.get_pattern() {
        Ok(p) => p,
        Err(_) => {
            log::debug!("[ItemContainer] Root does not support ItemContainerPattern, skipping");
            return Ok(None);
        }
    };

    // Step 2: Parse the XPath and extract a simple property we can search by
    let xpath_parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
    if xpath_parts.is_empty() {
        return Ok(None);
    }

    // Use the LAST step (the leaf we're actually looking for)
    let last_step = xpath_parts.last().unwrap();
    let parsed = parse_xpath_step(last_step);

    // Pick the best property for FindItemByProperty
    // Priority: AutomationId > Name > ClassName
    let (property, value) = if let Some(aid) = parsed.required_props.iter().find(|(k, _)| k.as_attr_name() == "AutomationId") {
        (UIProperty::AutomationId, aid.1.clone())
    } else if let Some(nm) = parsed.required_props.iter().find(|(k, _)| k.as_attr_name() == "Name") {
        (UIProperty::Name, nm.1.clone())
    } else if let Some(cn) = parsed.required_props.iter().find(|(k, _)| k.as_attr_name() == "ClassName") {
        (UIProperty::ClassName, cn.1.clone())
    } else {
        log::debug!("[ItemContainer] No simple property to search by in '{}'", last_step);
        return Ok(None);
    };

    log::info!("[ItemContainer] Trying FindItemByProperty({:?}='{}') ({}ms)",
        property, value, start.elapsed().as_millis());

    // Step 3: Call FindItemByProperty with null start_after (search from beginning)
    match container_pattern.find_first_item_by_property(property, Variant::from(value.as_str())) {
        Ok(elem) => {
            log::info!("[ItemContainer] ✓ Found element via FindItemByProperty({:?}='{}') ({}ms)",
                property, value, start.elapsed().as_millis());
            Ok(Some(elem))
        }
        Err(e) => {
            log::info!("[ItemContainer] FindItemByProperty({:?}='{}') not found ({}ms): {:?}",
                property, value, start.elapsed().as_millis(), e);
            Ok(None)
        }
    }
}

/// Chain FindAll: resolve multi-layer descendant XPath, collecting ALL matches at the last step.
/// Returns `ChainResult` indicating whether all steps resolved, partially resolved,
/// or Chain was not applicable.
pub(super) fn search_descendants_chain_find_all(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],
    filter: &FindAllFilter,
) -> ChainResult {
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
            if step_idx > 0 {
                return ChainResult::Partial(ChainProgress {
                    last_successful_step: step_idx - 1,
                    last_element: current_root,
                });
            }
            return ChainResult::NotApplicable;
        }

        let condition = match build_uia_condition_from_step(auto, &parsed) {
            Some(c) => c,
            None => {
                log::info!("[Chain FindAll] Step {}: cannot build UIA condition ({}ms)",
                    step_idx, chain_start.elapsed().as_millis());
                if step_idx > 0 {
                    return ChainResult::Partial(ChainProgress {
                        last_successful_step: step_idx - 1,
                        last_element: current_root,
                    });
                }
                return ChainResult::NotApplicable;
            }
        };
        let t_step = Instant::now();

        // Choose TreeScope based on parsed prefix — crucial to avoid matching self!
        // Subtree = Element + Descendants (includes self → broken for bare type steps like "Pane")
        // Children = direct children only (for / axis)
        // Descendants = all descendants excluding self (for // axis)
        let scope = match &parsed.prefix {
            XPathStepPrefix::Child => TreeScope::Children,
            XPathStepPrefix::Descendant | XPathStepPrefix::DepthLimited { .. } => TreeScope::Descendants,
        };
        let scope_name = match scope {
            TreeScope::Children => "Children",
            TreeScope::Descendants => "Descendants",
            _ => "Other",
        };

        if is_last {
            // Last step: use FindAll to collect all matches
            match current_root.find_all(scope, &condition) {
                Ok(elems) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindAll({}) found {} in {}ms", step_idx, scope_name, elems.len(), ms);

                    let results = filter_findall_results(root, elems, "ChainAll", filter);
                    if results.is_empty() {
                        log::info!("[Chain FindAll] Step {}: filtered to 0 results", step_idx);
                        return ChainResult::Complete(vec![]);
                    }

                    log::info!("[Chain FindAll] ✓ All {} steps resolved in {}ms [{}]",
                        xpath_parts.len(), chain_start.elapsed().as_millis(),
                        step_times.iter().map(|ms| format!("{}ms", ms)).collect::<Vec<_>>().join(", "));
                    return ChainResult::Complete(results);
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindAll({}) not found '{}' ({}ms): {:?}", step_idx, scope_name, step_str, ms, e);
                    return ChainResult::Complete(vec![]);
                }
            }
        } else {
            // Intermediate step: use FindFirst to narrow down
            match current_root.find_first(scope, &condition) {
                Ok(elem) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindFirst({}) found {} in {}ms", step_idx, scope_name, elem_summary(&elem), ms);
                    current_root = elem;
                }
                Err(e) => {
                    let ms = t_step.elapsed().as_millis();
                    step_times.push(ms);
                    log::info!("[Chain FindAll] Step {}: FindFirst({}) not found '{}' in {}ms: {:?}", step_idx, scope_name, step_str, ms, e);
                    if step_idx > 0 {
                        return ChainResult::Partial(ChainProgress {
                            last_successful_step: step_idx - 1,
                            last_element: current_root,
                        });
                    }
                    return ChainResult::Complete(vec![]);
                }
            }
        }
    }

    ChainResult::NotApplicable
}
