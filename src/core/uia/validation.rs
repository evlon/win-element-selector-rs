use super::*;

pub fn validate_xpath(
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
    log::info!("\n------------- begin validate_xpath -------------");
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
                hierarchy_refresh: vec![],
                found_elements: vec![],
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
            hierarchy_refresh: vec![],
            found_elements: vec![],
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
                    hierarchy_refresh: vec![],
                    found_elements: vec![],
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
                            // Log found element's runtimeId and rect
                            let first = &results[0];
                            let rid = first.get_runtime_id().ok().map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default();
                            let rect_str = first.get_bounding_rectangle().ok().map(|r| format!("({},{},{},{})", r.get_left(), r.get_top(), r.get_right(), r.get_bottom())).unwrap_or_default();
                            log::info!("[VAL][CHILD] Found element: rid=[{}] rect={} name='{}' ctrl='{}' (total={})",
                                rid, rect_str,
                                first.get_name().unwrap_or_default(),
                                first.get_control_type_raw().map(control_type_name).unwrap_or_default(),
                                results.len());
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
                            // 读取整条层级路径的最新属性用于刷新属性面板
                            let hierarchy_refresh = build_hierarchy_refresh(&results[0], &auto, hierarchy);
                            log::info!("[PERF][CHILD] ✓ Total child mode: {}ms", child_start.elapsed().as_millis());
                            return DetailedValidationResult {
                                overall,
                                segments,
                                layers,
                                total_duration_ms: total_start.elapsed().as_millis() as u64,
                                is_offscreen,
                                not_found_reason: None,
                                hierarchy_refresh,
                                found_elements: results,
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
            hierarchy_refresh: vec![],
            found_elements: vec![],
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
                    // Log found element's runtimeId and rect
                    let first = &results[0];
                    let rid = first.get_runtime_id().ok().map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default();
                    let rect_str = first.get_bounding_rectangle().ok().map(|r| format!("({},{},{},{})", r.get_left(), r.get_top(), r.get_right(), r.get_bottom())).unwrap_or_default();
                    log::info!("[VAL] Found element: rid=[{}] rect={} name='{}' ctrl='{}' (total={}, window={})",
                        rid, rect_str,
                        first.get_name().unwrap_or_default(),
                        first.get_control_type_raw().map(control_type_name).unwrap_or_default(),
                        results.len(), win_idx + 1);
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
                hierarchy_refresh: vec![],
                found_elements: vec![],
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

    // 读取整条层级路径的最新属性用于刷新属性面板
    let hierarchy_refresh = if !results.is_empty() {
        build_hierarchy_refresh(&results[0], &auto, hierarchy)
    } else {
        vec![]
    };

    DetailedValidationResult {
        overall,
        segments,
        layers,
        total_duration_ms: total_start.elapsed().as_millis() as u64,
        is_offscreen,
        not_found_reason,
        hierarchy_refresh,
        found_elements: results,
    }
}

/// 构建整条层级路径的刷新数据。
/// 遍历 hierarchy 的每一层，通过 found element 的祖先链找到对应的 UIElement，
/// 读取最新属性生成 HierarchyNode（用于刷新属性面板）。
fn build_hierarchy_refresh(
    found_elem: &UIElement,
    auto: &UIAutomation,
    hierarchy: &[HierarchyNode],
) -> Vec<Option<HierarchyNode>> {
    let first_match = UiaXPathElement::new(found_elem.clone().into(), auto.clone().into());
    let ancestors = first_match.ancestors();

    hierarchy.iter().enumerate().map(|(layer_idx, _)| {
        let actual_elem = if layer_idx == hierarchy.len() - 1 {
            Some(&first_match)
        } else {
            let ancestor_idx = ancestors.len().saturating_sub(layer_idx + 1);
            ancestors.get(ancestor_idx)
        };

        match actual_elem {
            Some(elem) => {
                let raw: IUIAutomationElement = elem.raw_element_clone();
                let uia_elem: UIElement = raw.into();
                element_to_node(&uia_elem, auto)
            }
            None => None,
        }
    }).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
