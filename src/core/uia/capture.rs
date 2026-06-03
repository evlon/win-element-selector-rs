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
    info!("[Normal] Starting capture at ({}, {})", x, y);

    // 1. Get the element at the point
    let point = UiaPoint::new(x, y);
    let target = auto.element_from_point(point)
        .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?;
    let target_name = target.get_name().unwrap_or_default();
    let target_ct = target.get_control_type_raw().map(control_type_name).unwrap_or_default();
    debug!("[Normal] ElementFromPoint: type='{}' name='{}'", target_ct, target_name);

    // 2. Build ancestor chain using ControlViewWalker (per design: normal capture uses ControlViewWalker).
    // ControlViewWalker filters out decorative/intermediate nodes, giving a shorter, faster chain.
    // For cross-HWND boundary detection, we still enumerate child HWNDs.
    let walker = auto.get_control_view_walker()
        .or_else(|_| auto.get_raw_view_walker())
        .map_err(|e| anyhow::anyhow!("ControlViewWalker: {e}"))?;
    let desktop = auto.get_root_element()?;

    let mut chain: Vec<UIElement> = vec![target.clone()];
    let mut current = walker.get_parent(&target).ok();
    while let Some(elem) = current {
        let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
        chain.push(elem.clone());
        if is_desktop {
            break;
        }
        current = walker.get_parent(&elem).ok();
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
        if let Ok(ctrl_walker) = auto.get_control_view_walker() {
            last.index = sibling_index(&target, &ctrl_walker).unwrap_or(0);
            if last.index > 0 {
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value   = last.index.to_string();
                    f.enabled = true;
                }
                last.sibling_count = count_siblings(&target, &ctrl_walker).unwrap_or(0);
            }
        }
    }

    // 5. Set walker hints for faster XPath validation
    let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
    set_walker_hints(&mut hierarchy, window_pid);

    // Extract window_info BEFORE truncation
    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-boundary via child HWND enumeration
    let mut capture_mode = CaptureMode::Fast;
    if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
        info!("[Normal] Cross-boundary detected at index {} (window_pid={}), switching to FastChild mode",
            boundary_idx, window_pid);
        hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
        capture_mode = CaptureMode::FastChild;
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
    let window_node_idx = (1..hierarchy.len())
        .find(|&i| hierarchy[i].process_id == window_pid)?;

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
    info!("[TRUNC_IN] hierarchy len={} boundary={} window_pid={}", hierarchy.len(), boundary_idx, window_pid);
    for (i, n) in hierarchy.iter().enumerate() {
        info!("[TRUNC_IN]   idx={} name='{}' class='{}' ctrl='{}' pid={} incl={}",
            i, n.name, n.class_name, n.control_type, n.process_id, n.included);
    }

    let window_node_idx = (1..hierarchy.len())
        .find(|&i| hierarchy[i].process_id == window_pid)
        .unwrap_or(0);

    info!("[TRUNC_IN] window_node_idx={} (ctrl='{}' name='{}')",
        window_node_idx, hierarchy[window_node_idx].control_type, hierarchy[window_node_idx].name);

    let window_node = hierarchy[window_node_idx].clone();

    if hierarchy.len() <= boundary_idx + 1 {
        info!("[TRUNC_IN] no content after boundary, returning window-only hierarchy");
        return vec![window_node];
    }
    let child_nodes: Vec<HierarchyNode> = hierarchy.drain(boundary_idx + 1..).collect();
    info!("[TRUNC_IN] after drain: remaining={} child_count={} (skipped boundary node at idx={})",
        hierarchy.len(), child_nodes.len(), boundary_idx);

    let mut result = vec![window_node];
    for (i, mut node) in child_nodes.into_iter().enumerate() {
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

fn do_capture_enhanced(auto: &UIAutomation, x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    debug!("[Enhanced] Starting at ({}, {})", x, y);

    // Step 1: ElementFromPoint to get the top-level element at cursor
    let point = UiaPoint::new(x, y);
    let mut hit_elem = auto.element_from_point(point)
        .map_err(|e| anyhow::anyhow!("ElementFromPoint: {}", e))?;
    let mut hit_name = hit_elem.get_name().unwrap_or_default();
    let mut _hit_ct = hit_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
    let mut hit_fwid = hit_elem.get_framework_id().unwrap_or_default();
    debug!("[Enhanced] ElementFromPoint: type='{}' name='{}' fwid='{}'", _hit_ct, hit_name, hit_fwid);

    let my_pid = std::process::id();

    // Step 1.1: If ElementFromPoint hit our own process (highlight/overlay window),
    // try to find the real target element beneath it
    if let Ok(pid) = hit_elem.get_process_id() {
        if pid == my_pid {
            info!("[Enhanced] ElementFromPoint hit own process (type='{}' name='{}'), searching for real target",
                _hit_ct, hit_name);
            let desktop = auto.get_root_element()?;
            let walker = match auto.get_raw_view_walker().ok()
                .or_else(|| auto.get_control_view_walker().ok()) {
                Some(w) => w,
                None => { info!("[Enhanced] No walker available, skipping self-process fix"); return Ok(CaptureResult { hierarchy: vec![], cursor_x: x, cursor_y: y, error: Some("无法获取 TreeWalker".into()), window_info: None, capture_mode: CaptureMode::Full }); }
            };
            let mut child = walker.get_first_child(&desktop).ok();
            while let Some(c) = child {
                if let Ok(c_pid) = c.get_process_id() {
                    if c_pid == my_pid {
                        child = walker.get_next_sibling(&c).ok();
                        continue;
                    }
                }
                if let Ok(c_rect) = c.get_bounding_rectangle() {
                    let sr = SimpleRect::from(&c_rect);
                    let cw = sr.width();
                    let ch = sr.height();
                    if cw > 0 && ch > 0 && point_in_rect(x, y, &sr) {
                        let c_name = c.get_name().unwrap_or_default();
                        let c_ct = c.get_control_type_raw().map(control_type_name).unwrap_or_default();
                        let c_fwid = c.get_framework_id().unwrap_or_default();
                        info!("[Enhanced] Found real target window: type='{}' name='{}' fwid='{}'",
                            c_ct, c_name, c_fwid);
                        hit_elem = c;
                        hit_name = c_name;
                        _hit_ct = c_ct;
                        hit_fwid = c_fwid;
                        break;
                    }
                }
                child = walker.get_next_sibling(&c).ok();
            }
        }
    }

    let hit_pid = hit_elem.get_process_id().unwrap_or(0);

    // Save the original hit element for fallback
    let original_hit_elem = hit_elem.clone();

    // Save pre-Step-1.5 element for FindAll retry
    let pre_cross_hwnd_elem = hit_elem.clone();
    let _pre_cross_hwnd_ct = _hit_ct.clone();
    let _pre_cross_hwnd_name = hit_name.clone();

    // Step 1.5: Cross-HWND probe
    let original_hit_fwid = hit_fwid.clone();

    // Find the window HWND by walking up from hit_elem
    let window_hwnd = (|| -> Option<HWND> {
        if let Ok(handle) = hit_elem.get_native_window_handle() {
            let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
            if !raw_handle.is_invalid() {
                return Some(HWND(raw_handle.0));
            }
        }
        let walker = auto.get_raw_view_walker().ok()
            .or_else(|| auto.get_control_view_walker().ok())?;
        let mut cur = walker.get_parent(&hit_elem).ok();
        while let Some(ancestor) = cur {
            if let Ok(handle) = ancestor.get_native_window_handle() {
                let raw_handle: windows::Win32::Foundation::HANDLE = handle.into();
                if !raw_handle.is_invalid() {
                    return Some(HWND(raw_handle.0));
                }
            }
            cur = walker.get_parent(&ancestor).ok();
        }
        None
    })();

    if let Some(hwnd) = window_hwnd {
        let child_hwnds = enum_child_hwnds(hwnd);
        if !child_hwnds.is_empty() {
            debug!("[Enhanced] Found {} child HWNDs, probing for cross-framework element", child_hwnds.len());
            for child_hwnd in &child_hwnds {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    let c_fwid = child_elem.get_framework_id().unwrap_or_default();
                    let c_ct_local = child_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                    if c_fwid == original_hit_fwid { continue; }
                    if let Ok(rect) = child_elem.get_bounding_rectangle() {
                        let sr = SimpleRect::from(&rect);
                        let w = sr.width();
                        let h = sr.height();
                        if w > 0 && h > 0 && point_in_rect(x, y, &sr) {
                            let c_name = child_elem.get_name().unwrap_or_default();
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
    let cross_hwnd_changed = !auto.compare_elements(&hit_elem, &pre_cross_hwnd_elem).unwrap_or(false);

    let raw_walker = auto.get_raw_view_walker()
        .or_else(|_| auto.get_control_view_walker())
        .map_err(|e| anyhow::anyhow!("RawViewWalker: {}", e))?;

    /// Inner BFS loop: walk the tree from `start_elem` level by level,
    /// returning the deepest element whose BoundingRectangle contains (x, y).
    fn bfs_find_deepest(
        walker: &UITreeWalker,
        start_elem: &UIElement,
        x: i32, y: i32,
        my_pid: u32,
    ) -> Option<UIElement> {
        let mut current = start_elem.clone();
        let mut visited_rids: HashSet<Vec<i32>> = HashSet::new();
        if let Ok(rid) = current.get_runtime_id() { visited_rids.insert(rid); }
        let mut depth: u32 = 0;

        loop {
            if depth > 30 { break; }

            let mut best_child: Option<UIElement> = None;
            let mut best_area = i64::MAX;
            let mut best_ct = String::new();
            let mut best_name = String::new();
            let mut best_is_leaf_with_name = false;

            let mut child = walker.get_first_child(&current).ok();
            while let Some(c) = child {
                // Skip elements from our own process
                if let Ok(pid) = c.get_process_id() {
                    if pid == my_pid {
                        child = walker.get_next_sibling(&c).ok();
                        continue;
                    }
                }
                // Skip offscreen elements
                if let Ok(offscreen) = c.is_offscreen() {
                    if offscreen {
                        child = walker.get_next_sibling(&c).ok();
                        continue;
                    }
                }
                // Cycle detection
                if let Ok(rid) = c.get_runtime_id() {
                    if visited_rids.contains(&rid) {
                        child = walker.get_next_sibling(&c).ok();
                        continue;
                    }
                }
                // Check BoundingRectangle contains the point
                let rect = match c.get_bounding_rectangle() {
                    Ok(r) => r,
                    Err(_) => { child = walker.get_next_sibling(&c).ok(); continue; }
                };
                let sr = SimpleRect::from(&rect);
                let w = sr.width();
                let h = sr.height();
                if w <= 0 || h <= 0 || !point_in_rect(x, y, &sr) {
                    child = walker.get_next_sibling(&c).ok();
                    continue;
                }

                let area = w as i64 * h as i64;
                let ct = c.get_control_type_raw().map(control_type_name).unwrap_or_default();
                let name = c.get_name().unwrap_or_default();
                let c_is_leaf_with_name = is_leaf_control_type(&ct) && !name.is_empty();

                debug!("[BFS]   depth={} match: type='{}' name='{}' area={} leaf_with_name={}",
                    depth, ct, name, area, c_is_leaf_with_name);

                let should_replace = if c_is_leaf_with_name && !best_is_leaf_with_name {
                    true
                } else if !c_is_leaf_with_name && best_is_leaf_with_name {
                    false
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

                child = walker.get_next_sibling(&c).ok();
            }

            match best_child {
                Some(c) => {
                    if let Ok(rid) = c.get_runtime_id() { visited_rids.insert(rid); }
                    debug!("[BFS] depth={} → type='{}' name='{}' area={}",
                        depth, best_ct, best_name, best_area);
                    current = c;
                    depth += 1;
                }
                None => break,
            }
        }

        if depth > 0 { Some(current) } else { None }
    }

    // Try BFS from the current hit_elem
    let mut target_elem = bfs_find_deepest(&raw_walker, &hit_elem, x, y, my_pid);

    // Step 2.1: Retry with pre-Step-1.5 element if BFS found nothing and cross-HWND changed
    if target_elem.is_none() && cross_hwnd_changed {
        info!("[Enhanced] BFS from cross-HWND element found no matches, retrying with pre-Step-1.5 element");
        target_elem = bfs_find_deepest(&raw_walker, &pre_cross_hwnd_elem, x, y, my_pid);
    }

    // Step 2.5: Same-process window enumeration
    if target_elem.is_none() && hit_pid != 0 && hit_pid != my_pid {
        info!("[Enhanced] Step 2.5: searching same-process windows (PID={})", hit_pid);
        let top_hwnds = enumerate_top_level_windows();
        let same_pid_hwnds: Vec<HWND> = top_hwnds.into_iter().filter(|hwnd| {
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut pid)) };
            pid == hit_pid
        }).collect();
        debug!("[Enhanced] Step 2.5: found {} top-level HWNDs for PID={}", same_pid_hwnds.len(), hit_pid);

        for top_hwnd in &same_pid_hwnds {
            let children = enum_child_hwnds(*top_hwnd);
            for child_hwnd in &children {
                if let Ok(child_elem) = auto.element_from_handle((*child_hwnd).into()) {
                    if let Ok(rect) = child_elem.get_bounding_rectangle() {
                        let sr = SimpleRect::from(&rect);
                        let w = sr.width();
                        let h = sr.height();
                        if w > 0 && h > 0 && point_in_rect(x, y, &sr) {
                            let c_ct = child_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                            let c_name = child_elem.get_name().unwrap_or_default();
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
    let mut target_elem = match target_elem {
        Some(e) => {
            debug!("[Enhanced] SELECTED via BFS: type='{}' name='{}'",
                e.get_control_type_raw().map(control_type_name).unwrap_or_default(),
                e.get_name().unwrap_or_default());
            e
        }
        None => {
            info!("[Enhanced] BFS found no matches, falling back to ElementFromPoint result");
            hit_elem = original_hit_elem.clone();
            let desktop = auto.get_root_element()?;
            let mut ch: Vec<UIElement> = vec![hit_elem.clone()];
            let mut cur = raw_walker.get_parent(&hit_elem).ok();
            while let Some(elem) = cur {
                let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
                ch.push(elem.clone());
                if is_desktop { break; }
                cur = raw_walker.get_parent(&elem).ok();
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

    // Step 4: Build ancestor chain using RawViewWalker
    let desktop = auto.get_root_element()?;

    let build_chain = |target: &UIElement, walker: &UITreeWalker, auto: &UIAutomation| -> Vec<UIElement> {
        let mut ch: Vec<UIElement> = vec![target.clone()];
        let mut cur = walker.get_parent(target).ok();
        while let Some(elem) = cur {
            let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
            ch.push(elem.clone());
            if is_desktop {
                break;
            }
            cur = walker.get_parent(&elem).ok();
        }
        ch.reverse();
        ch
    };

    let mut chain = build_chain(&target_elem, &raw_walker, &auto);
    debug!("[Enhanced] RawViewWalker chain length = {}", chain.len());

    // Step 4.5: Log chain info for diagnostics
    if chain.len() < 3 {
        info!("[Enhanced] Chain too short (len={}), falling back to normal capture chain", chain.len());
        chain = build_chain(&original_hit_elem, &raw_walker, &auto);
        target_elem = original_hit_elem.clone();
        info!("[Enhanced] Fallback chain length = {}", chain.len());
    } else {
        if let Some(win_elem) = chain.get(1) {
            let win_ct = win_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
            let win_name = win_elem.get_name().unwrap_or_default();
            let win_pid = win_elem.get_process_id().unwrap_or(0);
            info!("[Enhanced] Chain OK (len={}): window type='{}' name='{}' pid={}",
                chain.len(), win_ct, win_name, win_pid);
        }
    }

    // Step 5: Build hierarchy
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

    let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
    set_walker_hints(&mut hierarchy, window_pid);

    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-HWND boundary
    let mut capture_mode = CaptureMode::Full;
    if cross_hwnd_changed {
        info!("[Enhanced] Cross-HWND detected, switching to FullChild mode");
        if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
            hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
            capture_mode = CaptureMode::FullChild;
        }
    }
    let empty = String::new();
    let empty2 = String::new();
    let target_ct = hierarchy.last().map(|n| &n.control_type).unwrap_or(&empty);
    let target_name = hierarchy.last().map(|n| &n.name).unwrap_or(&empty2);
    let normal_ct = original_hit_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
    let normal_name = original_hit_elem.get_name().unwrap_or_default();
    info!("[Enhanced] hierarchy depth={} target='{}' name='{}' mode={:?} | ElementFromPoint type='{}' name='{}'",
        hierarchy.len(), target_ct, target_name, capture_mode, normal_ct, normal_name);

    Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info, capture_mode })
}

/// Enumerate child HWNDs of a given parent HWND
fn enum_child_hwnds(parent: HWND) -> Vec<HWND> {
    let mut children: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(enum_child_callback),
            LPARAM(&mut children as *mut _ as isize),
        );
    }
    children
}

extern "system" fn enum_child_callback(hwnd: HWND, lparam: LPARAM) -> WinBool {
    unsafe {
        let children = &mut *(lparam.0 as *mut Vec<HWND>);
        children.push(hwnd);
        WinBool(1)
    }
}

/// Enumerate all top-level windows
fn enumerate_top_level_windows() -> Vec<HWND> {
    let mut windows: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut windows as *mut _ as isize),
        );
    }
    windows
}

extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> WinBool {
    unsafe {
        let windows = &mut *(lparam.0 as *mut Vec<HWND>);
        if IsWindowVisible(hwnd).as_bool() {
            windows.push(hwnd);
        }
        WinBool(1)
    }
}
