use super::*;

pub fn validate_selector_and_xpath_detailed(
    window_selector: &str,
    element_xpath: &str,
    hierarchy: &[HierarchyNode],
) -> DetailedValidationResult {
    use std::time::Instant;
    let total_start = Instant::now();
    
    log::info!("[PERF] Starting validation for window_selector='{}' xpath='{}'", window_selector, element_xpath);
    
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
            overall: ValidationResult::Error(
                format!("窗口未找到: {}", window_selector)
            ),
            segments: vec![],
            layers: vec![],
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            is_offscreen: None,
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

    // ── Child mode check ──
    let (capture_mode, _) = CaptureMode::strip_xpath_prefix(element_xpath);
    let is_child_mode = capture_mode.map_or(false, |m| m.is_child_mode());

    if is_child_mode {
        log::info!("[Validate] Child mode detected, searching via EnumChildWindows");
        for search_root in &matched_windows {
            let hwnd = match search_root.get_native_window_handle() {
                Ok(h) => {
                    let raw: windows::Win32::Foundation::HANDLE = h.into();
                    HWND(raw.0)
                }
                Err(_) => continue,
            };
            let child_hwnds = enum_child_hwnds(hwnd);
            log::info!("[Validate] Child mode: {} child HWNDs", child_hwnds.len());

            for child_hwnd in &child_hwnds {
                let child_elem = match auto.element_from_handle((*child_hwnd).into()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match find_by_xpath_with_fallback(&auto, &child_elem, element_xpath) {
                    Ok((results, segments)) => {
                        if !results.is_empty() {
                            let overall = if results.is_empty() {
                                ValidationResult::NotFound
                            } else {
                                let mut rects = Vec::with_capacity(results.len());
                                for elem in &results {
                                    if let Ok(r) = elem.get_bounding_rectangle() {
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
                            } else { None };

                            let layers: Vec<LayerValidationResult> = Vec::new();
                            return DetailedValidationResult {
                                overall,
                                segments,
                                layers,
                                total_duration_ms: total_start.elapsed().as_millis() as u64,
                                is_offscreen,
                            };
                        }
                    }
                    Err(_) => continue,
                }
            }
        }
        return DetailedValidationResult {
            overall: ValidationResult::NotFound,
            segments: vec![],
            layers: vec![],
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            is_offscreen: None,
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

        if total_start.elapsed().as_millis() > 10000 {
            log::info!("[PERF] Total validation time exceeded 10s, stopping");
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

        match find_by_xpath_with_fallback(&auto, search_root, element_xpath) {
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
            };
        }
    };
    
    let overall = if results.is_empty() {
        ValidationResult::NotFound
    } else {
        let mut rects = Vec::with_capacity(results.len());
        for elem in &results {
            if let Ok(r) = elem.get_bounding_rectangle() {
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
    
    DetailedValidationResult {
        overall,
        segments,
        layers,
        total_duration_ms: total_start.elapsed().as_millis() as u64,
        is_offscreen,
    }
}
