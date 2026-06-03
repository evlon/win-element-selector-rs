use super::*;

pub fn capture_at_point(x: i32, y: i32) -> CaptureResult {
    match do_capture(x, y) {
        Ok(result) => result,
        Err(e) => {
            error!("capture_at_point({x},{y}) failed: {e}");
            CaptureResult {
                hierarchy: vec![],
                cursor_x: x, cursor_y: y,
                error: Some(format!("捕获失败: {}", e)),
                window_info: None,
                capture_mode: CaptureMode::Fast,
            }
        }
    }
}

fn do_capture(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    let auto = get_automation()?;
    let pt   = POINT { x, y };
    info!("[Normal] Starting capture at ({}, {})", x, y);

    // 1. Get the element at the point
    let target: IUIAutomationElement = unsafe {
        auto.ElementFromPoint(pt)
            .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?
    };
    let target_name = get_bstr(unsafe { target.CurrentName() });
    let target_ct = unsafe { target.CurrentControlType().map(control_type_name).unwrap_or_default() };
    debug!("[Normal] ElementFromPoint: type='{}' name='{}'", target_ct, target_name);

    // 2. Build ancestor chain using ControlViewWalker.
    // Use ControlViewWalker to match the [fast] validation path (strict ControlView only).
    // This ensures the captured hierarchy is exactly what ControlView sees,
    // avoiding mismatch between capture (RawView) and validation (ControlView).
    let walker = unsafe { auto.ControlViewWalker() }?;
    let desktop = unsafe { auto.GetRootElement()? };

    let mut chain: Vec<IUIAutomationElement> = vec![target.clone()];
    let mut current = unsafe { walker.GetParentElement(&target).ok() };
    while let Some(elem) = current {
        let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
        chain.push(elem.clone());
        if is_desktop {
            break;
        }
        current = unsafe { walker.GetParentElement(&elem).ok() };
    }
    chain.reverse(); // now: root → target

    // 3. Build hierarchy (root → target order)
    let window_index = 1;

    let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
    for (chain_idx, elem) in chain.iter().enumerate() {
        if let Some(mut node) = element_to_node(elem, &auto) {
            node.depth_from_window = chain_idx.saturating_sub(window_index);
            hierarchy.push(node);
        }
    }

    // 4. Compute sibling index for the target element
    if let Some(last) = hierarchy.last_mut() {
        last.is_target = true;
        let walker = unsafe { auto.ControlViewWalker().ok() };
        if let Some(ref w) = walker {
            last.index = sibling_index(&target, w).unwrap_or(0);
            if last.index > 0 {
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value   = last.index.to_string();
                    f.enabled = true;
                }
                last.sibling_count = count_siblings(&target, w).unwrap_or(0);
            }
        }
    }

    // 5. Set walker hints for faster XPath validation
    let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
    set_walker_hints(&mut hierarchy, window_pid);

    // Extract window_info BEFORE truncation (truncation removes Desktop, 
    // making hierarchy[0] the window instead of Desktop → index offset)
    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-boundary (target is in a child window/sub-process)
    // If any node after the Window has a different PID, the target is in a child window.
    let mut capture_mode = CaptureMode::Fast;
    if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
        info!("[Normal] Cross-boundary detected at index {} (window_pid={}), switching to FastChild mode",
            boundary_idx, window_pid);
        // Truncate hierarchy: keep actual window + nodes from boundary_idx+1
        hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
        capture_mode = CaptureMode::FastChild;
        // Debug: print truncated hierarchy
        for (i, n) in hierarchy.iter().enumerate() {
            info!("[TRUNCATED] idx={} name='{}' class='{}' ctrl='{}' depth={} included={}",
                i, n.name, n.class_name, n.control_type, n.depth_from_window, n.included);
        }
    }

    debug!("[Normal] Capture complete: hierarchy_depth={} target='{}' mode={:?}",
        hierarchy.len(), target_name, capture_mode);

    Ok(CaptureResult {
        hierarchy,
        cursor_x: x,
        cursor_y: y,
        error: None,
        window_info,
        capture_mode,
    })
}

fn find_cross_boundary_index(hierarchy: &[HierarchyNode], window_pid: u32) -> Option<usize> {
    // Find the actual window node: first node (after Desktop) that matches window_pid
    let window_node_idx = (1..hierarchy.len())
        .find(|&i| hierarchy[i].process_id == window_pid)?;

    // Search from the node AFTER the window for a cross-boundary node
    for i in (window_node_idx + 1)..hierarchy.len() {
        if hierarchy[i].process_id != window_pid && hierarchy[i].process_id != 0 {
            return Some(i);
        }
    }
    None
}

fn truncate_hierarchy_to_child(
    mut hierarchy: Vec<HierarchyNode>,
    boundary_idx: usize,
    window_pid: u32,
) -> Vec<HierarchyNode> {
    // Debug: print input hierarchy
    info!("[TRUNC_IN] hierarchy len={} boundary={} window_pid={}", hierarchy.len(), boundary_idx, window_pid);
    for (i, n) in hierarchy.iter().enumerate() {
        info!("[TRUNC_IN]   idx={} name='{}' class='{}' ctrl='{}' pid={} incl={}",
            i, n.name, n.class_name, n.control_type, n.process_id, n.included);
    }

    // Find the actual window node: first node (after Desktop) that matches window_pid
    let window_node_idx = (1..hierarchy.len())
        .find(|&i| hierarchy[i].process_id == window_pid)
        .unwrap_or(0);

    info!("[TRUNC_IN] window_node_idx={} (ctrl='{}' name='{}')",
        window_node_idx, hierarchy[window_node_idx].control_type, hierarchy[window_node_idx].name);

    // Clone BEFORE drain
    let window_node = hierarchy[window_node_idx].clone();

    // Skip the boundary node itself (e.g., Chrome_WidgetWin_0) —
    // it's the child window container, not content. Start from boundary_idx+1.
    if hierarchy.len() <= boundary_idx + 1 {
        info!("[TRUNC_IN] no content after boundary, returning window-only hierarchy");
        return vec![window_node];
    }
    let child_nodes: Vec<HierarchyNode> = hierarchy.drain(boundary_idx + 1..).collect();
    info!("[TRUNC_IN] after drain: remaining={} child_count={} (skipped boundary node at idx={})",
        hierarchy.len(), child_nodes.len(), boundary_idx);

    let mut result = vec![window_node];
    for (i, mut node) in child_nodes.into_iter().enumerate() {
        // First child node gets depth 0, subsequent nodes get depth i
        node.depth_from_window = i;
        result.push(node);
    }
    info!("[TRUNC_IN] result len={}", result.len());
    result
}

pub fn capture_enhanced_at_point(x: i32, y: i32) -> CaptureResult {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(e) => {
            return CaptureResult {
                hierarchy: vec![], cursor_x: x, cursor_y: y,
                error: Some(format!("COM 初始化失败: {}", e)),
                window_info: None,
                capture_mode: CaptureMode::Full,
            };
        }
    };
    match do_capture_enhanced(&auto, x, y) {
        Ok(result) => result,
        Err(e) => {
            error!("capture_enhanced_at_point({},{}) failed: {}", x, y, e);
            CaptureResult {
                hierarchy: vec![], cursor_x: x, cursor_y: y,
                error: Some(format!("增强捕获失败: {}", e)),
                window_info: None,
                capture_mode: CaptureMode::Full,
            }
        }
    }
}

fn do_capture_enhanced(auto: &IUIAutomation, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    let pt = POINT { x, y };
    debug!("[Enhanced] Starting at ({}, {})", x, y);

    // Step 1: ElementFromPoint to get the top-level element at cursor
    let mut hit_elem = unsafe {
        auto.ElementFromPoint(pt)
            .map_err(|e| anyhow::anyhow!("ElementFromPoint: {}", e))?
    };
    #[allow(unused_assignments)] // hit_name/_hit_ct updated in Steps 1.1/1.5 for diagnostics
    let mut hit_name = get_bstr(unsafe { hit_elem.CurrentName() });
    let mut _hit_ct = unsafe { hit_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
    let mut hit_fwid = get_bstr(unsafe { hit_elem.CurrentFrameworkId() });
    debug!("[Enhanced] ElementFromPoint: type='{}' name='{}' fwid='{}'", _hit_ct, hit_name, hit_fwid);

    let my_pid = std::process::id();

    // Step 1.1: If ElementFromPoint hit our own process (highlight/overlay window),
    // try to find the real target element beneath it by walking the Desktop children
    // and finding the deepest non-self element whose BoundingRectangle contains the point.
    if let Ok(pid) = unsafe { hit_elem.CurrentProcessId() } {
        if pid as u32 == my_pid {
            info!("[Enhanced] ElementFromPoint hit own process (type='{}' name='{}'), searching for real target",
                _hit_ct, hit_name);
            let desktop = unsafe { auto.GetRootElement()? };
            let walker = match unsafe { auto.RawViewWalker().ok() }
                .or_else(|| unsafe { auto.ControlViewWalker().ok() }) {
                Some(w) => w,
                None => { info!("[Enhanced] No walker available, skipping self-process fix"); return Ok(CaptureResult { hierarchy: vec![], cursor_x: x, cursor_y: y, error: Some("无法获取 TreeWalker".into()), window_info: None, capture_mode: CaptureMode::Full }); }
            };
            // Enumerate Desktop children to find the topmost non-self window containing the point
            let mut child = unsafe { walker.GetFirstChildElement(&desktop).ok() };
            while let Some(c) = child {
                if let Ok(c_pid) = unsafe { c.CurrentProcessId() } {
                    if c_pid as u32 == my_pid {
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                        continue;
                    }
                }
                if let Ok(c_rect) = unsafe { c.CurrentBoundingRectangle() } {
                    let cw = c_rect.right - c_rect.left;
                    let ch = c_rect.bottom - c_rect.top;
                    if cw > 0 && ch > 0 && point_in_rect(x, y, &c_rect) {
                        // Found a non-self window containing the point; use it
                        // Try to get a deeper element at the same point via FindAll
                        let c_name = get_bstr(unsafe { c.CurrentName() });
                        let c_ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                        let c_fwid = get_bstr(unsafe { c.CurrentFrameworkId() });
                        info!("[Enhanced] Found real target window: type='{}' name='{}' fwid='{}'",
                            c_ct, c_name, c_fwid);
                        hit_elem = c;
                        hit_name = c_name;
                        _hit_ct = c_ct;
                        hit_fwid = c_fwid;
                        break;
                    }
                }
                child = unsafe { walker.GetNextSiblingElement(&c).ok() };
            }
        }
    }

    // PID of the (possibly Step-1.1-corrected) hit element.
    // Used by Step 2.5 to enumerate same-process windows when BFS fails.
    let hit_pid = unsafe { hit_elem.CurrentProcessId() }
        .map(|p| p as u32)
        .unwrap_or(0);

    // Save the original hit element for fallback (normal capture chain).
    // Must be after Step 1.1 so we save the corrected element (not our own overlay).
    let original_hit_elem = hit_elem.clone();

    // Save pre-Step-1.5 element for FindAll retry.
    // Step 1.5 may replace hit_elem with a cross-HWND element whose BoundingRectangle
    // covers the cursor but whose subtree has no matching elements at that point
    // (e.g., WeChat's Chrome HWND covers the entire window but cursor is in Qt area).
    // In that case, we retry FindAll with the pre-Step-1.5 element.
    let pre_cross_hwnd_elem = hit_elem.clone();
    let _pre_cross_hwnd_ct = _hit_ct.clone();
    let _pre_cross_hwnd_name = hit_name.clone();

    // Step 1.5: Cross-HWND probe
    // In apps like WeChat, ElementFromPoint returns a Qt element, but the real
    // target is inside a Chrome/WebView child HWND. The Chrome element is NOT a
    // UIA descendant of the Qt element, so FindAll(Subtree) and RawViewWalker
    // drill-down cannot reach it. We enumerate child HWNDs of the window,
    // find ones from a different framework that contain the cursor point, and
    // use their root element as the starting point instead.
    let original_hit_fwid = hit_fwid.clone();

    // Find the window HWND by walking up from hit_elem
    let window_hwnd = (|| -> Option<HWND> {
        // Try the hit element itself first
        if let Ok(h) = unsafe { hit_elem.CurrentNativeWindowHandle() } {
            return Some(HWND(h.0));
        }
        // Walk up ancestors to find one with a window handle
        let walker = unsafe { auto.RawViewWalker().ok() }
            .or_else(|| unsafe { auto.ControlViewWalker().ok() })?;
        let mut cur = unsafe { walker.GetParentElement(&hit_elem).ok() };
        while let Some(ancestor) = cur {
            if let Ok(h) = unsafe { ancestor.CurrentNativeWindowHandle() } {
                return Some(HWND(h.0));
            }
            cur = unsafe { walker.GetParentElement(&ancestor).ok() };
        }
        None
    })();

    if let Some(hwnd) = window_hwnd {
        let child_hwnds = enum_child_hwnds(hwnd);
        if !child_hwnds.is_empty() {
            debug!("[Enhanced] Found {} child HWNDs, probing for cross-framework element", child_hwnds.len());
            for child_hwnd in &child_hwnds {
                if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                    let c_fwid = get_bstr(unsafe { child_elem.CurrentFrameworkId() });
                    let c_ct_local = unsafe { child_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                    // Skip same-framework children (Qt → Qt is not a cross-framework transition)
                    if c_fwid == original_hit_fwid { continue; }
                    // Check if this child element contains the cursor point
                    if let Ok(rect) = unsafe { child_elem.CurrentBoundingRectangle() } {
                        let w = rect.right - rect.left;
                        let h = rect.bottom - rect.top;
                        if w > 0 && h > 0 && point_in_rect(x, y, &rect) {
                            let c_name = get_bstr(unsafe { child_elem.CurrentName() });
                            debug!("[Enhanced] Cross-HWND: child type='{}' name='{}' fwid='{}' contains point — using as hit element",
                                c_ct_local, c_name, c_fwid);
                            hit_elem = child_elem;
                            _hit_ct = c_ct_local;
                            break;
                        }
                    }
                }
            }
        }
    } else {
        debug!("[Enhanced] Step 1.5: no window handle found, skipping cross-HWND probe");
    }

    // Step 2: BFS-style traversal from hit_elem to find the deepest element at the cursor point.
    //
    // Instead of FindAll (which returns a flat list of ALL descendants and requires
    // area-based heuristics to estimate depth), we walk the RawViewWalker tree
    // level by level. At each level, we scan the direct children and pick the one
    // whose BoundingRectangle contains the cursor point. Then we descend into that
    // child and repeat, until no child contains the point.
    //
    // This naturally encodes depth — each BFS level IS one tree level — so we don't
    // need area as a depth proxy. When multiple siblings contain the point, we apply
    // leaf preference (leaf with name > container without name) then smallest area.
    //
    // Compared to the old FindAll + drill-down approach:
    // - FindAll(Subtree) returns a flat array; depth is unknown → area heuristic
    //   needed → tiny Group overlays beat large Text elements (BUG)
    // - Drill-down started from the (possibly wrong) FindAll result → too late
    // - BFS starts from the top and walks down → depth is inherent in the traversal
    let cross_hwnd_changed = !unsafe { auto.CompareElements(&hit_elem, &pre_cross_hwnd_elem)
        .unwrap_or(windows::core::BOOL(0)).as_bool() };

    let raw_walker = unsafe { auto.RawViewWalker() }
        .or_else(|_| unsafe { auto.ControlViewWalker() })
        .map_err(|e| anyhow::anyhow!("RawViewWalker: {}", e))?;

    /// Inner BFS loop: walk the tree from `start_elem` level by level,
    /// returning the deepest element whose BoundingRectangle contains (x, y).
    fn bfs_find_deepest(
        walker: &IUIAutomationTreeWalker,
        start_elem: &IUIAutomationElement,
        x: i32, y: i32,
        my_pid: u32,
    ) -> Option<IUIAutomationElement> {
        let mut current = start_elem.clone();
        let mut visited_rids: HashSet<Vec<i32>> = HashSet::new();
        if let Some(rid) = runtime_id_key(&current) { visited_rids.insert(rid); }
        let mut depth: u32 = 0;

        loop {
            if depth > 30 { break; } // Safety: prevent infinite loop

            let mut best_child: Option<IUIAutomationElement> = None;
            let mut best_area = i64::MAX;
            let mut best_ct = String::new();
            let mut best_name = String::new();
            let mut best_is_leaf_with_name = false;

            let mut child = unsafe { walker.GetFirstChildElement(&current).ok() };
            while let Some(c) = child {
                // Skip elements from our own process (highlight/overlay windows)
                if let Ok(pid) = unsafe { c.CurrentProcessId() } {
                    if pid as u32 == my_pid {
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                        continue;
                    }
                }
                // Skip offscreen elements
                if let Ok(offscreen) = unsafe { c.CurrentIsOffscreen() } {
                    if offscreen.0 != 0 {
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                        continue;
                    }
                }
                // Cycle detection
                if let Some(rid) = runtime_id_key(&c) {
                    if visited_rids.contains(&rid) {
                        child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                        continue;
                    }
                }
                // Check BoundingRectangle contains the point
                let rect = match unsafe { c.CurrentBoundingRectangle() } {
                    Ok(r) => r,
                    Err(_) => { child = unsafe { walker.GetNextSiblingElement(&c).ok() }; continue; }
                };
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;
                if w <= 0 || h <= 0 || !point_in_rect(x, y, &rect) {
                    child = unsafe { walker.GetNextSiblingElement(&c).ok() };
                    continue;
                }

                let area = w as i64 * h as i64;
                let ct = unsafe { c.CurrentControlType().map(control_type_name).unwrap_or_default() };
                let name = get_bstr(unsafe { c.CurrentName() });
                let c_is_leaf_with_name = is_leaf_control_type(&ct) && !name.is_empty();

                debug!("[BFS]   depth={} match: type='{}' name='{}' area={} leaf_with_name={}",
                    depth, ct, name, area, c_is_leaf_with_name);

                // Selection among siblings at this level:
                // 1. Leaf with name beats container without name (leaf preference)
                //    — fixes tiny Group overlays that beat larger Text elements
                // 2. Smallest area (most specific element)
                //    — for equal-area containers (Qt/Chrome intermediate Group/Pane),
                //      pick any; the next BFS level will refine further
                let should_replace = if c_is_leaf_with_name && !best_is_leaf_with_name {
                    true  // Leaf preference: leaf with name beats container without name
                } else if !c_is_leaf_with_name && best_is_leaf_with_name {
                    false // Don't replace leaf with container
                } else {
                    area < best_area
                };

                if should_replace {
                    best_child = Some(c.clone());
                    best_area = area;
                    best_ct = ct;
                    best_name = name;
                    best_is_leaf_with_name = c_is_leaf_with_name;
                }

                child = unsafe { walker.GetNextSiblingElement(&c).ok() };
            }

            match best_child {
                Some(c) => {
                    if let Some(rid) = runtime_id_key(&c) { visited_rids.insert(rid); }
                    debug!("[BFS] depth={} → type='{}' name='{}' area={}",
                        depth, best_ct, best_name, best_area);
                    current = c;
                    depth += 1;
                }
                None => break, // No child contains the point → current is deepest
            }
        }

        if depth > 0 { Some(current) } else { None }
    }

    // Try BFS from the current hit_elem (which may have been changed by Step 1.5)
    let mut target_elem = bfs_find_deepest(&raw_walker, &hit_elem, x, y, my_pid);

    // Step 2.1: Retry with pre-Step-1.5 element if BFS found nothing and
    // Step 1.5 changed hit_elem (cross-HWND scenario).
    // This handles the case where WeChat's Chrome HWND covers the entire window
    // but the cursor is in the Qt area — the Chrome element's subtree has no
    // matching elements, so we retry with the Qt element.
    if target_elem.is_none() && cross_hwnd_changed {
        info!("[Enhanced] BFS from cross-HWND element found no matches, retrying with pre-Step-1.5 element");
        target_elem = bfs_find_deepest(&raw_walker, &pre_cross_hwnd_elem, x, y, my_pid);
    }

    // Step 2.5: Same-process window enumeration.
    // If BFS found nothing, the target element might be in a different HWND tree
    // within the same process (e.g., a multi-window app where ElementFromPoint
    // returned a top-level container, but the real element is in a child HWND
    // of a different top-level window of the same process).
    // We enumerate all top-level windows belonging to hit_elem's PID,
    // then their child HWNDs, to find the most specific HWND containing (x,y).
    if target_elem.is_none() && hit_pid != 0 && hit_pid != my_pid {
        info!("[Enhanced] Step 2.5: searching same-process windows (PID={})", hit_pid);
        let top_hwnds = enumerate_top_level_windows();
        // Filter to windows belonging to the same PID
        let same_pid_hwnds: Vec<HWND> = top_hwnds.into_iter().filter(|hwnd| {
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut pid)) };
            pid == hit_pid
        }).collect();
        debug!("[Enhanced] Step 2.5: found {} top-level HWNDs for PID={}", same_pid_hwnds.len(), hit_pid);

        for top_hwnd in &same_pid_hwnds {
            let children = enum_child_hwnds(*top_hwnd);
            for child_hwnd in &children {
                if let Ok(child_elem) = unsafe { auto.ElementFromHandle(*child_hwnd) } {
                    if let Ok(rect) = unsafe { child_elem.CurrentBoundingRectangle() } {
                        let w = rect.right - rect.left;
                        let h = rect.bottom - rect.top;
                        if w > 0 && h > 0 && point_in_rect(x, y, &rect) {
                            let c_ct = unsafe { child_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
                            let c_name = get_bstr(unsafe { child_elem.CurrentName() });
                            debug!("[Enhanced] Step 2.5: child HWND type='{}' name='{}' contains point — trying BFS",
                                c_ct, c_name);
                            target_elem = bfs_find_deepest(&raw_walker, &child_elem, x, y, my_pid);
                            if target_elem.is_some() {
                                info!("[Enhanced] Step 2.5: found element via same-process child HWND");
                                break;
                            }
                        }
                    }
                }
            }
            if target_elem.is_some() { break; }
        }
    }

    // If no matching element found after all retries, fall back to original_hit_elem
    // (the ElementFromPoint result) — equivalent to normal capture, which is always reliable.
    let mut target_elem = match target_elem {
        Some(e) => {
            debug!("[Enhanced] SELECTED via BFS: type='{}' name='{}'",
                unsafe { e.CurrentControlType().map(control_type_name).unwrap_or_default() },
                get_bstr(unsafe { e.CurrentName() }));
            e
        }
        None => {
            info!("[Enhanced] BFS found no matches, falling back to ElementFromPoint result");
            hit_elem = original_hit_elem.clone();
            let desktop = unsafe { auto.GetRootElement()? };
            let mut ch: Vec<IUIAutomationElement> = vec![hit_elem.clone()];
            let mut cur = unsafe { raw_walker.GetParentElement(&hit_elem).ok() };
            while let Some(elem) = cur {
                let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
                ch.push(elem.clone());
                if is_desktop { break; }
                cur = unsafe { raw_walker.GetParentElement(&elem).ok() };
            }
            ch.reverse();
            let window_index = 1;
            let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(ch.len());
            for (chain_idx, elem) in ch.iter().enumerate() {
                if let Some(mut node) = element_to_node(elem, &auto) {
                    node.depth_from_window = chain_idx.saturating_sub(window_index);
                    hierarchy.push(node);
                }
            }
            if let Some(last) = hierarchy.last_mut() {
                last.is_target = true;
                last.index = sibling_index(&hit_elem, &raw_walker).unwrap_or(0);
                if last.index > 0 {
                    if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                        f.value = last.index.to_string(); f.enabled = true;
                    }
                    last.sibling_count = count_siblings(&hit_elem, &raw_walker).unwrap_or(0);
                }
            }
            let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
            set_walker_hints(&mut hierarchy, window_pid);
            let window_info = extract_window_info(&hierarchy);
            info!("[Enhanced] Fallback hierarchy depth={}", hierarchy.len());
            return Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info, capture_mode: CaptureMode::Full });
        }
    };

    // Step 4: Build ancestor chain using RawViewWalker (same walker from drill-down)
    // Always use RawViewWalker (not FindAll(Ancestors)) because FindAll uses
    // Control View which filters out intermediate elements (e.g. Qt Group nodes).
    // This ensures the ancestor chain is complete and matches the XPath search side.
    let desktop = unsafe { auto.GetRootElement()? };

    let build_chain = |target: &IUIAutomationElement| -> Vec<IUIAutomationElement> {
        let mut ch: Vec<IUIAutomationElement> = vec![target.clone()];
        let mut cur = unsafe { raw_walker.GetParentElement(target).ok() };
        while let Some(elem) = cur {
            let is_desktop = unsafe { auto.CompareElements(&elem, &desktop).unwrap_or(windows::core::BOOL(0)).as_bool() };
            ch.push(elem.clone());
            if is_desktop {
                break;
            }
            cur = unsafe { raw_walker.GetParentElement(&elem).ok() };
        }
        ch.reverse();
        ch
    };

    let mut chain = build_chain(&target_elem);
    debug!("[Enhanced] RawViewWalker chain length = {}", chain.len());

    // Step 4.5: Log chain info for diagnostics.
    // Previous PID-based validation was too strict and caused false fallbacks.
    // The PID filtering in FindAll (Step 3) and drill-down (Step 3.5) already
    // prevents selecting elements from our own process. If the chain is
    // suspiciously short (< 3 = Desktop + Window + target), fall back.
    if chain.len() < 3 {
        info!("[Enhanced] Chain too short (len={}), falling back to normal capture chain", chain.len());
        chain = build_chain(&original_hit_elem);
        target_elem = original_hit_elem.clone();
        info!("[Enhanced] Fallback chain length = {}", chain.len());
    } else {
        // Log chain[1] (the window element) for diagnostics
        if let Some(win_elem) = chain.get(1) {
            let win_ct = unsafe { win_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
            let win_name = get_bstr(unsafe { win_elem.CurrentName() });
            let win_pid = unsafe { win_elem.CurrentProcessId().unwrap_or(0) };
            info!("[Enhanced] Chain OK (len={}): window type='{}' name='{}' pid={}",
                chain.len(), win_ct, win_name, win_pid);
        }
    }

    // Step 5: Build hierarchy (same structure as normal capture)
    let window_index = 1;
    let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
    for (chain_idx, elem) in chain.iter().enumerate() {
        if let Some(mut node) = element_to_node(elem, &auto) {
            node.depth_from_window = chain_idx.saturating_sub(window_index);
            debug!("[Enhanced]   hierarchy[{}]: type='{}' name='{}' depth={}",
                chain_idx, node.control_type, node.name, node.depth_from_window);
            hierarchy.push(node);
        }
    }

    if let Some(last) = hierarchy.last_mut() {
        last.is_target = true;
        last.index = sibling_index(&target_elem, &raw_walker).unwrap_or(0);
        if last.index > 0 {
            if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                f.value = last.index.to_string(); f.enabled = true;
            }
            last.sibling_count = count_siblings(&target_elem, &raw_walker).unwrap_or(0);
        }
    }

    // Set walker hints for faster XPath validation
    let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
    set_walker_hints(&mut hierarchy, window_pid);

    // Extract window_info BEFORE truncation (truncation removes Desktop)
    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-HWND boundary from enhanced capture Step 1.5
    let mut capture_mode = CaptureMode::Full;
    if cross_hwnd_changed {
        info!("[Enhanced] Cross-HWND detected, switching to FullChild mode");
        // Find boundary index: first node with different PID from window
        if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
            hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
            capture_mode = CaptureMode::FullChild;
        }
    }
    let empty = String::new();
    let empty2 = String::new();
    let target_ct = hierarchy.last().map(|n| &n.control_type).unwrap_or(&empty);
    let target_name = hierarchy.last().map(|n| &n.name).unwrap_or(&empty2);
    let normal_ct = unsafe { original_hit_elem.CurrentControlType().map(control_type_name).unwrap_or_default() };
    let normal_name = get_bstr(unsafe { original_hit_elem.CurrentName() });
    info!("[Enhanced] hierarchy depth={} target='{}' name='{}' mode={:?} | ElementFromPoint type='{}' name='{}'",
        hierarchy.len(), target_ct, target_name, capture_mode, normal_ct, normal_name);

    Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info, capture_mode })
}

