use super::*;

pub fn validate_selector_and_xpath_detailed(
    window_selector: &str,
    element_xpath: &str,
    hierarchy: &[HierarchyNode],
    search_context: Option<&SearchContext>,
    timeout_ms: Option<u64>,
    chrome_treewalker_fallback: bool,
) -> DetailedValidationResult {
    use std::time::Instant;
    let total_start = Instant::now();

    // Compute effective timeout: Fast=1500ms, Full=3000ms, default=5000ms
    let effective_timeout = timeout_ms.unwrap_or_else(|| {
        match search_context.map(|ctx| ctx.locate_mode) {
            Some(LocateMode::Fast) | Some(LocateMode::FastChild) => 1500,
            Some(LocateMode::Full) | Some(LocateMode::FullChild) => 3000,
            _ => 5000,
        }
    });
    
    log::info!("[PERF] Starting validation for window_selector='{}' xpath='{}' timeout={}ms", window_selector, element_xpath, effective_timeout);
    
    let auto = match get_automation() {
        Ok(a)  => a,
        Err(e) => {
            log::error!("[PERF] Failed to get automation instance: {}", e);
            return DetailedValidationResult {
                overall: ValidationResult::Error(e.to_string()),
                segments: vec![],
                layers: vec![],
                total_duration_ms: total_start.elapsed().as_millis() as u64,
                is_offscreen: None,
                not_found_reason: None,
            };
        }
    };

    // Stage 1: Find all target windows using window selector
    log::info!("[PERF] Stage 1/2: Locating window with selector: {}", window_selector);
    let stage1_start = Instant::now();
    
    let mut matched_windows = find_window_by_selector(&auto, window_selector);
    
    let stage1_duration = stage1_start.elapsed().as_millis();
    log::info!("[PERF] Stage 1 completed in {}ms, found {} windows", stage1_duration, matched_windows.len());
    
    if matched_windows.is_empty() {
        return DetailedValidationResult {
            overall: ValidationResult::NotFound {
                reason: Some(NotFoundReason::WindowNotFound),
            },
            segments: vec![],
            layers: vec![],
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            is_offscreen: None,
            not_found_reason: Some(NotFoundReason::WindowNotFound),
        };
    }
    
    log::info!("[PERF] ✓ Found {} matching window(s), trying XPath on each", matched_windows.len());

    // ── Prioritize the foreground window ──
    {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let fg_hwnd = unsafe { GetForegroundWindow() };
        if !fg_hwnd.is_invalid() {
            let fg_idx = matched_windows.iter().position(|w| {
                w.get_native_window_handle().map(|h| {
                    let raw: windows::Win32::Foundation::HANDLE = h.into();
                    HWND(raw.0) == fg_hwnd
                }).unwrap_or(false)
            });
            if let Some(idx) = fg_idx {
                if idx > 0 {
                    log::info!("[PERF] Foreground window is at index {}, moving to front", idx);
                    let fg_win = matched_windows.remove(idx);
                    matched_windows.insert(0, fg_win);
                }
            }
        }
    }

    // ── Determine locate mode from search_context or xpath prefix ──
    let (locate_mode_from_prefix, prefix_hint, _) = LocateMode::strip_xpath_prefix(element_xpath);
    let locate_mode = search_context.map(|ctx| ctx.locate_mode)
        .or(locate_mode_from_prefix);
    let is_child_mode = locate_mode.map_or(false, |m| m.is_child_mode());

    // ── Resolve child HWND hint ──
    // Priority: search_context.child_hwnd_hint > hint from XPath prefix ([fast-child @ClassName='...'])
    let child_hwnd_hint = search_context.and_then(|ctx| ctx.child_hwnd_hint.clone())
        .or(prefix_hint);

    if is_child_mode {
        let child_start = Instant::now();
        log::info!("[PERF][CHILD] Child mode detected, searching via EnumChildWindows");
        if let Some(ref hint) = child_hwnd_hint {
            log::info!("[PERF][CHILD] Child HWND hint: class='{}' title='{}'", hint.hwnd_class, hint.hwnd_title);
        }
        for search_root in &matched_windows {
            let hwnd = match search_root.get_native_window_handle() {
                Ok(h) => {
                    let raw: windows::Win32::Foundation::HANDLE = h.into();
                    HWND(raw.0)
                }
                Err(_) => continue,
            };
            let t0 = Instant::now();
            let child_hwnds = enum_child_hwnds(hwnd);
            log::info!("[PERF][CHILD] enum_child_hwnds: {} HWNDs in {}ms", child_hwnds.len(), t0.elapsed().as_millis());

            // If we have a child_hwnd_hint, filter to matching child HWNDs only
            // Use element_from_handle_build_cache to prefetch ClassName + Name (2-3x faster)
            let t1 = Instant::now();
            let validation_cache = create_validation_cache_request(&auto);
            let auto_for_filter = auto.clone();
            let filtered_hwnds: Vec<HWND> = if let Some(ref hint) = child_hwnd_hint {
                let hint_class = hint.hwnd_class.clone();
                let hint_title = hint.hwnd_title.clone();
                child_hwnds.into_iter().filter(move |&ch| {
                    let elem_result = match &validation_cache {
                        Some(cr) => auto_for_filter.element_from_handle_build_cache(ch.into(), cr),
                        None => auto_for_filter.element_from_handle(ch.into()),
                    };
                    if let Ok(elem) = elem_result {
                        let class_matches = if validation_cache.is_some() {
                            elem.get_cached_classname()
                        } else {
                            elem.get_classname()
                        }.map(|c| c.contains(&hint_class)).unwrap_or(false);
                        let title_matches = if hint_title.is_empty() {
                            true
                        } else {
                            if validation_cache.is_some() {
                                elem.get_cached_name()
                            } else {
                                elem.get_name()
                            }.map(|n| n.contains(&hint_title)).unwrap_or(false)
                        };
                        class_matches && title_matches
                    } else {
                        false
                    }
                }).collect()
            } else {
                child_hwnds
            };
            log::info!("[PERF][CHILD] Filter HWNDs: {} filtered in {}ms", filtered_hwnds.len(), t1.elapsed().as_millis());

            if filtered_hwnds.is_empty() && child_hwnd_hint.is_some() {
                let hint_class = child_hwnd_hint.as_ref().unwrap().hwnd_class.clone();
                log::info!("[PERF][CHILD] No child HWND matched the hint, reporting ChildHwndNotFound ({}ms)", child_start.elapsed().as_millis());
                return DetailedValidationResult {
                    overall: ValidationResult::NotFound {
                        reason: Some(NotFoundReason::ChildHwndNotFound {
                            class: hint_class.clone(),
                        }),
                    },
                    segments: vec![],
                    layers: vec![],
                    total_duration_ms: total_start.elapsed().as_millis() as u64,
                    is_offscreen: None,
                    not_found_reason: Some(NotFoundReason::ChildHwndNotFound {
                        class: hint_class,
                    }),
                };
            }

            for (hwnd_idx, child_hwnd) in filtered_hwnds.iter().enumerate() {
                let t_elem = Instant::now();
                let child_elem = match auto.element_from_handle((*child_hwnd).into()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                log::info!("[PERF][CHILD] HWND[{}] element_from_handle: {}ms", hwnd_idx, t_elem.elapsed().as_millis());
                
                // Child HWND filtering is done via prefix attributes (e.g. @ClassName).
                // The XPath is used as-is — it's the user's target search path.
                let child_xpath = element_xpath;
                
                let t_find = Instant::now();
                match execute_xpath_steps_filtered(&auto, &child_elem, &child_xpath, &FindAllFilter::default(), Some(effective_timeout), chrome_treewalker_fallback) {
                    Ok((results, segments)) => {
                        log::info!("[PERF][CHILD] HWND[{}] execute_xpath_steps: {}ms, {} results", 
                            hwnd_idx, t_find.elapsed().as_millis(), results.len());
                        if !results.is_empty() {
                            // Use BuildCache to batch-prefetch BoundingRectangle + IsOffscreen
                            let t_rect = Instant::now();
                            let result_cache = create_bfs_cache_request(&auto);
                            let mut rects = Vec::with_capacity(results.len());
                            for elem in &results {
                                let rect = match &result_cache {
                                    Some(cr) => elem.build_updated_cache(cr).ok()
                                        .and_then(|ce| ce.get_cached_bounding_rectangle().ok()),
                                    None => elem.get_bounding_rectangle().ok(),
                                };
                                if let Some(r) = rect {
                                    rects.push(ElementRect {
                                        x: r.get_left(), y: r.get_top(),
                                        width: r.get_right() - r.get_left(), height: r.get_bottom() - r.get_top(),
                                    });
                                }
                            }
                            log::info!("[PERF][CHILD] Get rects for {} results: {}ms", results.len(), t_rect.elapsed().as_millis());
                            let first_rect = rects.first().cloned();
                            let overall = ValidationResult::Found { count: results.len(), first_rect, rects };
                            let is_offscreen = Some(results[0].is_offscreen().unwrap_or(false));

                            let layers: Vec<LayerValidationResult> = Vec::new();
                            log::info!("[PERF][CHILD] ✓ Total child mode: {}ms", child_start.elapsed().as_millis());
                            return DetailedValidationResult {
                                overall,
                                segments,
                                layers,
                                total_duration_ms: total_start.elapsed().as_millis() as u64,
                                is_offscreen,
                                not_found_reason: None,
                            };
                        }
                    }
                    Err(_) => continue,
                }
            }
        }
        log::info!("[PERF][CHILD] ✗ Child mode exhausted, returning NotFound ({}ms)", child_start.elapsed().as_millis());
        return DetailedValidationResult {
            overall: ValidationResult::NotFound {
                reason: Some(NotFoundReason::ElementGone),
            },
            segments: vec![],
            layers: vec![],
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            is_offscreen: None,
            not_found_reason: Some(NotFoundReason::ElementGone),
        };
    }
    // ── End child mode ──

    let mut last_error: Option<String> = None;
    let mut best_result: Option<(Vec<UIElement>, Vec<SegmentValidationResult>)> = None;
    let mut webview_window_tried = false;

    const MAX_WINDOWS_TO_TRY: usize = 5;
    let windows_to_try = matched_windows.len().min(MAX_WINDOWS_TO_TRY);
    if matched_windows.len() > MAX_WINDOWS_TO_TRY {
        log::info!("[PERF] Limiting window search to first {} of {} windows (foreground window is first)",
            MAX_WINDOWS_TO_TRY, matched_windows.len());
    }

    for (win_idx, search_root) in matched_windows.iter().enumerate() {
        if win_idx >= windows_to_try {
            log::info!("[PERF] Reached window limit ({}), stopping", MAX_WINDOWS_TO_TRY);
            break;
        }

        if total_start.elapsed().as_millis() > effective_timeout as u128 {
            log::info!("[PERF] Total validation time exceeded {}ms, stopping", effective_timeout);
            break;
        }

        log::info!("[PERF] Stage 2/2: Trying XPath on window {} of {}", win_idx + 1, windows_to_try);
        let stage2_window_start = Instant::now();

        // ── Fast skip: WebView windows ──
        {
            let win_class = search_root.get_classname().unwrap_or_default();
            if is_webview_class(&win_class) {
                if webview_window_tried {
                    log::info!("[PERF] Skipping WebView window {} class='{}' (already tried a WebView window)", 
                        win_idx + 1, win_class);
                    continue;
                }
                webview_window_tried = true;
                log::info!("[PERF] Trying first WebView window {} class='{}' (subsequent WebView windows will be skipped)", 
                    win_idx + 1, win_class);
            }
        }

        // Debug-only: print window's direct children tree for diagnostics
        #[cfg(debug_assertions)]
        if win_idx == 0 {
            log::info!("[XPath Validation] Window's direct children (RawViewWalker):");
            let walker = auto.get_raw_view_walker().ok()
                .or_else(|| auto.get_control_view_walker().ok());
            if let Some(walker) = walker {
                let mut child = walker.get_first_child(search_root).ok();
                let mut idx = 0;
                while let Some(c) = child {
                    let ct = c.get_control_type_raw().map(control_type_name).unwrap_or_default();
                    let class = c.get_classname().unwrap_or_default();
                    let name = c.get_name().unwrap_or_default();
                    let fwid = c.get_framework_id().unwrap_or_default();
                    log::info!("  child[{}] {} class='{}' name='{}' frameworkId='{}'", idx, ct, class, name, fwid);

                    if idx == 0 {
                        log::info!("  first child's sub-children:");
                        let mut sub_child = walker.get_first_child(&c).ok();
                        let mut sub_idx = 0;
                        while let Some(sc) = sub_child {
                            let sub_ct = sc.get_control_type_raw().map(control_type_name).unwrap_or_default();
                            let sub_class = sc.get_classname().unwrap_or_default();
                            let sub_name = sc.get_name().unwrap_or_default();
                            let sub_fwid = sc.get_framework_id().unwrap_or_default();
                            log::info!("    sub[{}] {} class='{}' name='{}' frameworkId='{}'", sub_idx, sub_ct, sub_class, sub_name, sub_fwid);
                            sub_child = walker.get_next_sibling(&sc).ok();
                            sub_idx += 1;
                            if sub_idx > 5 { break; }
                        }
                    }

                    child = walker.get_next_sibling(&c).ok();
                    idx += 1;
                    if idx > 10 { break; }
                }
            }
        }

        match execute_xpath_steps_filtered(&auto, search_root, element_xpath, &FindAllFilter::default(), Some(effective_timeout), chrome_treewalker_fallback) {
            Ok((results, segments)) => {
                let window_duration = stage2_window_start.elapsed().as_millis();
                if !results.is_empty() {
                    log::info!("[PERF] ✓ Window {} XPath succeeded in {}ms, found {} results", win_idx + 1, window_duration, results.len());
                    best_result = Some((results, segments));
                    break;
                }
                log::info!("[PERF] Window {} - XPath matched 0 elements in {}ms, trying next window", win_idx + 1, window_duration);
                if best_result.is_none() {
                    best_result = Some((results, segments));
                }
            }
            Err(e) => {
                let window_duration = stage2_window_start.elapsed().as_millis();
                log::info!("[PERF] Window {} - XPath error in {}ms: {}, trying next window", win_idx + 1, window_duration, e);
                last_error = Some(e.to_string());
            }
        }
    }

    let (results, segments) = match best_result {
        Some(r) => r,
        None => {
            return DetailedValidationResult {
                overall: ValidationResult::Error(
                    last_error.unwrap_or_else(|| "XPath 执行失败".to_string())
                ),
                segments: vec![],
                layers: vec![],
                total_duration_ms: total_start.elapsed().as_millis() as u64,
                is_offscreen: None,
                not_found_reason: None,
            };
        }
    };
    
    let overall = if results.is_empty() {
        let reason = if total_start.elapsed().as_millis() >= effective_timeout as u128 {
            Some(NotFoundReason::Timeout {
                budget_ms: effective_timeout,
                elapsed_ms: total_start.elapsed().as_millis() as u64,
            })
        } else {
            // ── 从 segment_results 提取失败原因（需求 §7.2）──
            extract_not_found_reason_from_segments(&segments)
        };
        ValidationResult::NotFound { reason }
    } else {
        // Use BuildCache to batch-prefetch BoundingRectangle
        let result_cache = create_bfs_cache_request(&auto);
        let mut rects = Vec::with_capacity(results.len());
        for elem in &results {
            let rect = match &result_cache {
                Some(cr) => elem.build_updated_cache(cr).ok()
                    .and_then(|ce| ce.get_cached_bounding_rectangle().ok()),
                None => elem.get_bounding_rectangle().ok(),
            };
            if let Some(r) = rect {
                rects.push(ElementRect {
                    x: r.get_left(), y: r.get_top(),
                    width: r.get_right() - r.get_left(), height: r.get_bottom() - r.get_top(),
                });
            }
        }
        let first_rect = rects.first().cloned();
        ValidationResult::Found { count: results.len(), first_rect, rects }
    };

    let is_offscreen = if !results.is_empty() {
        Some(results[0].is_offscreen().unwrap_or(false))
    } else {
        None
    };
    
    // 生成逐层校验结果（复用 uiauto-xpath 的 ancestors 和 get_property API）
    let layers = if !results.is_empty() {
        let first_match = UiaXPathElement::new(results[0].clone().into(), auto.clone().into());
        let ancestors = first_match.ancestors();
        
        hierarchy.iter().enumerate().map(|(layer_idx, node)| {
            let actual_elem = if layer_idx == hierarchy.len() - 1 {
                Some(&first_match)
            } else {
                let ancestor_idx = ancestors.len().saturating_sub(layer_idx + 1);
                ancestors.get(ancestor_idx)
            };
            
            match actual_elem {
                Some(elem) => {
                    let props: Vec<PropertyValidationResult> = node.filters.iter().map(|f| {
                        let actual = elem.get_property(&f.name);
                        let matched = actual.as_ref().map_or(false, |act| {
                            match f.operator {
                                Operator::Equals => act == &f.value,
                                Operator::NotEquals => act != &f.value,
                                Operator::Contains => act.contains(&f.value),
                                Operator::NotContains => !act.contains(&f.value),
                                Operator::StartsWith => act.starts_with(&f.value),
                                Operator::NotStartsWith => !act.starts_with(&f.value),
                                Operator::EndsWith => act.ends_with(&f.value),
                                Operator::NotEndsWith => !act.ends_with(&f.value),
                                Operator::Matches => {
                                    Regex::new(&f.value).map_or(false, |re| re.is_match(act))
                                }
                                Operator::NotMatches => {
                                    Regex::new(&f.value).map_or(true, |re| !re.is_match(act))
                                }
                                _ => act == &f.value,
                            }
                        });
                        PropertyValidationResult {
                            attr_name: f.name.clone(),
                            operator: f.operator.clone(),
                            expected_value: f.value.clone(),
                            actual_value: actual,
                            matched,
                            enabled: f.enabled,
                        }
                    }).collect();
                    let all_matched = props.iter().all(|p| p.matched || !p.enabled);
                    LayerValidationResult {
                        node_index: layer_idx,
                        control_type: node.control_type.clone(),
                        node_label: node.tree_label(),
                        matched: all_matched,
                        properties: props,
                        match_count: 1,
                        duration_ms: 0,
                    }
                }
                None => LayerValidationResult {
                    node_index: layer_idx,
                    control_type: node.control_type.clone(),
                    node_label: node.tree_label(),
                    matched: false,
                    properties: node.filters.iter().map(|f| PropertyValidationResult {
                        attr_name: f.name.clone(),
                        operator: f.operator.clone(),
                        expected_value: f.value.clone(),
                        actual_value: None,
                        matched: false,
                        enabled: f.enabled,
                    }).collect(),
                    match_count: 0,
                    duration_ms: 0,
                },
            }
        }).collect()
    } else {
        vec![]
    };
    
    // Extract not_found_reason from overall if it's NotFound
    let not_found_reason = match &overall {
        ValidationResult::NotFound { reason } => reason.clone(),
        _ => None,
    };

    DetailedValidationResult {
        overall,
        segments,
        layers,
        total_duration_ms: total_start.elapsed().as_millis() as u64,
        is_offscreen,
        not_found_reason,
    }
}

/// 从 segment_results 中提取 NotFoundReason（需求 §7.2）。
///
/// 遍历 segments，找到最后一个匹配失败的步骤，根据 predicate_failures 判断：
/// - 如果 attr_name == "findOne" → `LeafNotUnique`
/// - 如果 attr_name == "Timeout" → `Timeout`（已在调用方检查，此处作为 safety net）
/// - 否则 → `StepNotFound { step, xpath_step }`
fn extract_not_found_reason_from_segments(
    segments: &[SegmentValidationResult],
) -> Option<NotFoundReason> {
    // 找到最后一个 matched == false 的 segment
    let failed_segment = segments.iter().rev().find(|s| !s.matched)?;

    // 检查 predicate_failures 判断具体失败类型
    for pf in &failed_segment.predicate_failures {
        if pf.attr_name == "findOne" {
            // 从 actual_value 中解析 candidates 数量
            let candidates = pf.actual_value.as_ref()
                .and_then(|v| v.split_whitespace().next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(failed_segment.match_count.max(2));
            return Some(NotFoundReason::LeafNotUnique { candidates });
        }
        if pf.attr_name == "Timeout" {
            // Timeout 已在调用方通过 elapsed 检查，这里作为 safety net
            return Some(NotFoundReason::Timeout {
                budget_ms: 0,
                elapsed_ms: failed_segment.duration_ms,
            });
        }
    }

    // 默认：步骤未找到
    Some(NotFoundReason::StepNotFound {
        step: failed_segment.segment_index + 1, // 1-based for display
        xpath_step: failed_segment.segment_text.clone(),
    })
}

/// Parse and strip the first element step from an XPath expression.
///
/// In child mode validation, the first XPath step describes the boundary sub-HWND.
/// We strip the first step so the search starts from the sub-HWND's first *real*
/// child element (because `element_from_handle` already lands at the boundary node).
///
/// Supports both old and new prefix formats:
/// - Old: `[fast-child]/Pane[@ClassName='...']/Document/Text` → `[fast-child]/Document/Text`
/// - New: `[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Text` → `[fast-child]/Text`
///
/// Hint extraction is DEPRECATED from this function — use `LocateMode::strip_xpath_prefix`
/// instead which returns hint directly from the new prefix format.
///
/// Returns `(deprecated_hint, remaining_xpath)`.
/// The deprecated hint is only populated for backward-compat old format.
///
/// Examples:
/// - `[fast-child]/Pane[@ClassName='Chrome_WidgetWin_0']/Document/Text`
///   → `(Some(ChildHwndHint { class: "Chrome_WidgetWin_0" }), "[fast-child]/Document/Text")`
/// - `[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Group/Text`
///   → `(None, "[fast-child]/Group/Text")`
/// - `/Pane/Document/Group` → `(None, "/Document/Group")`
#[allow(dead_code)]
pub(super) fn parse_first_xpath_step(xpath: &str) -> (Option<ChildHwndHint>, String) {
    let xpath = xpath.trim();
    if xpath.is_empty() {
        return (None, String::new());
    }

    let bytes = xpath.as_bytes();
    let mut pos = 0;

    // Step 0: Skip capture mode prefix, handling both old and new formats
    // Old: [fast-child], [full-child], [fast], [full]
    // New: [fast-child @ClassName='...'], [full-child @ClassName='...']
    // When the prefix contains attributes (new format), the remaining XPath is already
    // the target search path — do NOT strip anything further.
    let (prefix_str, is_new_format) = if bytes.starts_with(b"[fast-child") {
        let after = skip_child_prefix_attrs(bytes, 0);
        let is_new = bytes[0..after].contains(&b'@');
        pos = after;
        ("[fast-child]", is_new)
    } else if bytes.starts_with(b"[full-child") {
        let after = skip_child_prefix_attrs(bytes, 0);
        let is_new = bytes[0..after].contains(&b'@');
        pos = after;
        ("[full-child]", is_new)
    } else if bytes.starts_with(b"[fast]") {
        pos = b"[fast]".len();
        ("[fast]", false)
    } else if bytes.starts_with(b"[full]") {
        pos = b"[full]".len();
        ("[full]", false)
    } else {
        ("", false)
    };

    // ★ New format: prefix already contains child HWND hints (e.g. [fast-child @ClassName='...']).
    // The remaining XPath is the user's actual target search path — do NOT strip anything.
    if is_new_format {
        let rest = xpath[pos..].trim();
        let remaining = if rest.is_empty() {
            prefix_str.to_string()
        } else if rest.starts_with('/') {
            // Preserve // or / as-is from user's XPath
            format!("{}{}", prefix_str, rest)
        } else {
            format!("{}/{}", prefix_str, rest)
        };
        return (None, remaining);
    }

    // ── Old format handling below ──
    // Step 1: Skip leading /
    while pos < bytes.len() && bytes[pos] == b'/' {
        pos += 1;
    }

    if pos >= bytes.len() {
        return (None, String::new());
    }

    let first_step_start = pos;

    // Step 2: Skip the control type name (letters, digits, underscore)
    while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
        pos += 1;
    }

    let first_step_end = pos;
    let first_step_bytes = &bytes[first_step_start..first_step_end];

    // Step 3: Extract predicate content (handle nested brackets)
    let predicate_text = if pos < bytes.len() && bytes[pos] == b'[' {
        let mut depth = 1;
        let pred_start = pos + 1; // skip opening '['
        pos += 1;
        while pos < bytes.len() && depth > 0 {
            match bytes[pos] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            pos += 1;
        }
        let pred_end = pos - 1; // exclude closing ']'
        Some(std::str::from_utf8(&bytes[pred_start..pred_end]).unwrap_or(""))
    } else {
        None
    };

    // Step 4: If there's a positional predicate like [1] after the step, skip it too
    if pos < bytes.len() && bytes[pos] == b'[' {
        let mut depth = 1;
        pos += 1;
        while pos < bytes.len() && depth > 0 {
            match bytes[pos] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            pos += 1;
        }
    }

    let after_first_step = xpath[pos..].trim().to_string();

    // ── Extract ChildHwndHint from the first step (backward-compat old format) ──
    // New format: hint is in the prefix, not here. This extraction is for old format only.
    let hint = extract_child_hint_from_step(
        std::str::from_utf8(first_step_bytes).unwrap_or(""),
        predicate_text,
    );

    // ★ Preserve the locate mode prefix so downstream (execute_xpath_steps_filtered)
    // can still detect [fast-child]/[full-child] and apply the correct strategy.
    // For new format, prefix_len already includes the attrs, but we emit clean "[fast-child]".
    let remaining = if !prefix_str.is_empty() {
        let trimmed = after_first_step.trim_start_matches('/');
        if trimmed.is_empty() {
            prefix_str.to_string()
        } else {
            format!("{}/{}", prefix_str, trimmed)
        }
    } else {
        after_first_step
    };

    (hint, remaining)
}

/// Skip past a child mode prefix including optional attributes.
/// Input starts at the `[` of `[fast-child @ClassName='...']`.
/// Returns position right after the closing `]`.
#[allow(dead_code)]
fn skip_child_prefix_attrs(bytes: &[u8], start: usize) -> usize {
    let mut pos = start;
    // Skip prefix base like [fast-child or [full-child
    while pos < bytes.len() && bytes[pos] != b' ' && bytes[pos] != b']' {
        pos += 1;
    }
    // Skip attributes if present
    while pos < bytes.len() && bytes[pos] == b' ' {
        pos += 1;
    }
    if pos < bytes.len() && bytes[pos] == b'@' {
        // Skip to closing ]
        while pos < bytes.len() && bytes[pos] != b']' {
            pos += 1;
        }
    }
    // Skip past closing ]
    if pos < bytes.len() && bytes[pos] == b']' {
        pos += 1;
    }
    pos
}

/// Extract ChildHwndHint from the first XPath step's control type + predicates.
///
/// Looks for `@ClassName='value'` or `starts-with(@ClassName, 'value')` in the predicate.
#[allow(dead_code)]
fn extract_child_hint_from_step(
    _control_type: &str,
    predicate: Option<&str>,
) -> Option<ChildHwndHint> {
    let pred = predicate?;
    if pred.is_empty() {
        return None;
    }

    // Try exact match: @ClassName='...'
    for quote in ['\'', '"'] {
        let marker = format!("@ClassName={}", quote);
        if let Some(start) = pred.find(&marker) {
            let val_start = start + marker.len();
            if let Some(end) = pred[val_start..].find(quote) {
                let class = &pred[val_start..val_start + end];
                if !class.is_empty() {
                    return Some(ChildHwndHint {
                        hwnd_class: class.to_string(),
                        hwnd_title: String::new(),
                    });
                }
            }
        }
    }

    // Try starts-with: starts-with(@ClassName, '...')
    for quote in ['\'', '"'] {
        let marker = format!("starts-with(@ClassName, {}", quote);
        if let Some(start) = pred.find(&marker) {
            let val_start = start + marker.len();
            if let Some(end) = pred[val_start..].find(quote) {
                let class = &pred[val_start..val_start + end];
                if !class.is_empty() {
                    return Some(ChildHwndHint {
                        hwnd_class: class.to_string(),
                        hwnd_title: String::new(),
                    });
                }
            }
        }
    }

    None
}

/// No-op: child HWND filtering is now done exclusively via prefix attributes
/// (e.g. `[fast-child @ClassName='...']` from `LocateMode::strip_xpath_prefix`).
/// The remaining XPath is always the user's target search path and should not be modified.
#[allow(dead_code)]
pub(super) fn strip_first_xpath_step(xpath: &str) -> String {
    xpath.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── strip_first_xpath_step: now always returns input as-is ───
    // Child HWND filtering is done via prefix attributes from strip_xpath_prefix.
    // The remaining XPath is the user's target search path — never modified.

    #[test]
    fn test_strip_first_xpath_step_simple() {
        assert_eq!(
            strip_first_xpath_step("/Pane/Document/Group"),
            "/Pane/Document/Group"
        );
    }

    #[test]
    fn test_strip_first_xpath_step_with_predicate() {
        let input = "/Pane[starts-with(@ClassName, 'Chrome_Widget')]/Document[@AutomationId='73457920' and starts-with(@ClassName, 'Chrome_RenderWidgetHost')]/Group/Text[@Name='hello']";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    #[test]
    fn test_strip_first_xpath_step_with_fast_child_prefix() {
        let input = "[fast-child]/Pane[starts-with(@ClassName, 'Chrome_Widget')]/Document[@AutomationId='73457920' and starts-with(@ClassName, 'Chrome_RenderWidgetHost')]/Group/Text[@Name='一生的六个阶段，你正在经历哪一个？']";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    #[test]
    fn test_strip_first_xpath_step_with_full_child_prefix() {
        let input = "[full-child]/Pane[starts-with(@ClassName, 'Chrome_Widget')]/Document/Text";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    #[test]
    fn test_strip_first_xpath_step_with_fast_prefix() {
        let input = "[fast]/Pane/Document/Text";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    #[test]
    fn test_strip_first_xpath_step_descendant() {
        let input = "//Pane[starts-with(@ClassName, 'Chrome_Widget')]/Document/Text";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    #[test]
    fn test_strip_first_xpath_step_single_step() {
        assert_eq!(strip_first_xpath_step("/Document"), "/Document");
    }

    #[test]
    fn test_strip_first_xpath_step_no_prefix_no_step() {
        assert_eq!(strip_first_xpath_step("Document/Text"), "Document/Text");
    }

    #[test]
    fn test_strip_first_xpath_step_only_prefix() {
        assert_eq!(strip_first_xpath_step("[fast-child]"), "[fast-child]");
    }

    #[test]
    fn test_strip_first_xpath_step_empty() {
        assert_eq!(strip_first_xpath_step(""), "");
    }

    #[test]
    fn test_strip_new_format_classname() {
        let input = "[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Group/Text";
        assert_eq!(strip_first_xpath_step(input), input);
    }

    // ─── New: parse_first_xpath_step tests ───

    #[test]
    fn test_parse_first_step_exact_class() {
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child]/Pane[@ClassName='Chrome_WidgetWin_0']/Document/Text"
        );
        assert_eq!(remaining, "[fast-child]/Document/Text");
        assert!(hint.is_some());
        let h = hint.unwrap();
        assert_eq!(h.hwnd_class, "Chrome_WidgetWin_0");
        assert_eq!(h.hwnd_title, "");
    }

    #[test]
    fn test_parse_first_step_starts_with_class() {
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child]/Pane[starts-with(@ClassName, 'Chrome_Widget')]/Document/Text"
        );
        assert_eq!(remaining, "[fast-child]/Document/Text");
        assert!(hint.is_some());
        let h = hint.unwrap();
        assert_eq!(h.hwnd_class, "Chrome_Widget");
    }

    #[test]
    fn test_parse_first_step_no_class() {
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child]/Pane[@Name='hello']/Document/Text"
        );
        assert_eq!(remaining, "[fast-child]/Document/Text");
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_first_step_no_prefix() {
        // Without prefix, remaining unchanged (no prefix to preserve)
        let (hint, remaining) = parse_first_xpath_step(
            "/Pane[@ClassName='Chrome_WidgetWin_0']/Document/Text"
        );
        assert_eq!(remaining, "/Document/Text");
        assert!(hint.is_some());
        let h = hint.unwrap();
        assert_eq!(h.hwnd_class, "Chrome_WidgetWin_0");
    }

    #[test]
    fn test_parse_first_step_single_step() {
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child]/Pane[@ClassName='Chrome_WidgetWin_0']"
        );
        // Single step with prefix → remaining is just the prefix.
        // This is a degenerate case (sub-HWND is the target itself).
        assert_eq!(remaining, "[fast-child]");
        assert!(hint.is_some());
        assert_eq!(hint.unwrap().hwnd_class, "Chrome_WidgetWin_0");
    }

    #[test]
    fn test_parse_first_step_empty() {
        let (hint, remaining) = parse_first_xpath_step("");
        assert_eq!(remaining, "");
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_first_step_no_predicate() {
        let (hint, remaining) = parse_first_xpath_step("[fast-child]/Pane/Document/Text");
        assert_eq!(remaining, "[fast-child]/Document/Text");
        assert!(hint.is_none());
    }

    // ─── New format: [fast-child @ClassName='...'] ───
    // In new format, the prefix already contains the child HWND hint.
    // The remaining XPath is the user's target search path — do NOT strip anything.

    #[test]
    fn test_parse_first_step_new_format_classname() {
        // New format: hint in prefix, remaining is the user's actual search path
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Group/Text"
        );
        assert_eq!(remaining, "[fast-child]/Document/Group/Text");
        // Hint is NOT extracted here (it's in prefix, not first step)
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_first_step_new_format_no_first_step() {
        // New format, target element after prefix
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child @ClassName='Chrome_WidgetWin_0']/Text"
        );
        assert_eq!(remaining, "[fast-child]/Text");
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_first_step_new_format_descendant() {
        // New format with // descendant path — should preserve the //Text
        let (hint, remaining) = parse_first_xpath_step(
            "[fast-child @ClassName='Chrome_WidgetWin_0']//Text[@Name='hello']"
        );
        assert_eq!(remaining, "[fast-child]//Text[@Name='hello']");
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_first_step_new_format_full_child() {
        // Same for [full-child] with attrs
        let (hint, remaining) = parse_first_xpath_step(
            "[full-child @ClassName='Chrome_WidgetWin_0']/Document/Text"
        );
        assert_eq!(remaining, "[full-child]/Document/Text");
        assert!(hint.is_none());
    }

    // ─── strip_xpath_prefix new format tests ───

    #[test]
    fn test_strip_prefix_new_format_classname() {
        let (mode, hint, rest) = LocateMode::strip_xpath_prefix(
            "[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Text"
        );
        assert_eq!(mode, Some(LocateMode::FastChild));
        assert!(hint.is_some());
        let h = hint.unwrap();
        assert_eq!(h.hwnd_class, "Chrome_WidgetWin_0");
        assert_eq!(h.hwnd_title, "");
        assert_eq!(rest, "/Document/Text");
    }

    #[test]
    fn test_strip_prefix_new_format_name() {
        let (mode, hint, rest) = LocateMode::strip_xpath_prefix(
            "[fast-child @Name='hello']/Text"
        );
        assert_eq!(mode, Some(LocateMode::FastChild));
        assert!(hint.is_some());
        let h = hint.unwrap();
        assert_eq!(h.hwnd_class, "");
        assert_eq!(h.hwnd_title, "hello");
        assert_eq!(rest, "/Text");
    }

    #[test]
    fn test_strip_prefix_new_format_no_attrs() {
        // Old format without attrs — still works
        let (mode, hint, rest) = LocateMode::strip_xpath_prefix(
            "[fast-child]/Pane/Document/Text"
        );
        assert_eq!(mode, Some(LocateMode::FastChild));
        assert!(hint.is_none());
        assert_eq!(rest, "/Pane/Document/Text");
    }

    #[test]
    fn test_strip_prefix_new_format_full_child() {
        let (mode, hint, rest) = LocateMode::strip_xpath_prefix(
            "[full-child @ClassName='Chrome_WidgetWin_1']/Text"
        );
        assert_eq!(mode, Some(LocateMode::FullChild));
        assert!(hint.is_some());
        assert_eq!(hint.unwrap().hwnd_class, "Chrome_WidgetWin_1");
        assert_eq!(rest, "/Text");
    }

    #[test]
    fn test_strip_prefix_fast_no_attrs() {
        // [fast] — no attrs (non-child mode)
        let (mode, hint, rest) = LocateMode::strip_xpath_prefix(
            "[fast]/Group/Button"
        );
        assert_eq!(mode, Some(LocateMode::Fast));
        assert!(hint.is_none());
        assert_eq!(rest, "/Group/Button");
    }
}
