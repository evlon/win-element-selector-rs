use super::*;
use uiauto_xpath::control_type_id_to_name;

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
                capture_mode: CaptureMode::Normal,
                locate_mode: LocateMode::Fast,
                search_context: SearchContext::default_fast(),
            }
        }
    }
}

fn do_capture(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
    let auto = get_automation()?;
    let capture_start = std::time::Instant::now();
    info!("[Normal] Starting capture at ({}, {})", x, y);

    // 1. Get the element at the point
    let t1 = std::time::Instant::now();
    let point = UiaPoint::new(x, y);
    let hit_elem = auto.element_from_point(point)
        .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?;
    let hit_name = hit_elem.get_name().unwrap_or_default();
    let hit_ct = hit_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
    log::info!("[PERF][CAP] ElementFromPoint: {}ms (type='{}' name='{}')", t1.elapsed().as_millis(), hit_ct, hit_name);

    // 1.5. Light BFS to find the deepest leaf element at (x,y)
    // This solves the issue where ElementFromPoint returns a container (Group/Pane)
    // instead of the actual target element.
    let t15 = std::time::Instant::now();
    let target = light_bfs_to_leaf(&auto, &hit_elem, x, y, 5)
        .unwrap_or_else(|| hit_elem.clone());
    let target_name = target.get_name().unwrap_or_default();
    let target_ct = target.get_control_type_raw().map(control_type_name).unwrap_or_default();
    if !auto.compare_elements(&target, &hit_elem).unwrap_or(false) {
        log::info!("[PERF][CAP] BFS found deeper target: {}ms (type='{}' name='{}')", t15.elapsed().as_millis(), target_ct, target_name);
    } else {
        log::info!("[PERF][CAP] BFS no deeper target: {}ms", t15.elapsed().as_millis());
    }

    // 2. Build ancestor chain using ControlViewWalker (per design: normal capture uses ControlViewWalker).
    let t2 = std::time::Instant::now();
    let mut walker_is_control_view = true;
    let walker = match auto.get_control_view_walker() {
        Ok(w) => w,
        Err(_) => {
            walker_is_control_view = false;
            log::warn!("[CAP] ControlViewWalker unavailable, falling back to RawViewWalker");
            auto.get_raw_view_walker()
                .map_err(|e| anyhow::anyhow!("RawViewWalker: {e}"))?
        }
    };
    log::info!("[PERF][CAP] Walker type: {}", if walker_is_control_view { "ControlView" } else { "RawView (FALLBACK)" });
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
    log::info!("[PERF][CAP] Ancestor chain: {} nodes in {}ms", chain.len(), t2.elapsed().as_millis());

    // 3. Build hierarchy (root → target order)
    let window_index = 1;

    // Use BuildCache for batch property prefetch (5-10x faster than individual COM calls)
    let t3 = std::time::Instant::now();
    let cache_request = create_hierarchy_cache_request(&auto);
    let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
    for (chain_idx, elem) in chain.iter().enumerate() {
        let node = if let Some(ref cr) = cache_request {
            elem.build_updated_cache(cr).ok()
                .and_then(|cached_elem| element_to_node_cached(&cached_elem))
        } else {
            element_to_node(elem, &auto)
        };
        if let Some(mut node) = node {
            node.depth_from_window = chain_idx.saturating_sub(window_index);
            hierarchy.push(node);
        }
    }
    log::info!("[PERF][CAP] Build hierarchy: {} nodes in {}ms", hierarchy.len(), t3.elapsed().as_millis());

    // 4. Compute sibling index for the target element
    // Skip sibling index computation when target is inside an embedded browser (Chrome WebView),
    // because traversing thousands of DOM siblings is extremely slow (5-7s) and
    // sibling index is unreliable for web content anyway.
    let t4 = std::time::Instant::now();
    // Read parent info before mutable borrow
    let parent_class_fw = if hierarchy.len() >= 2 {
        let parent = &hierarchy[hierarchy.len() - 2];
        Some((parent.class_name.clone(), parent.framework_id.clone()))
    } else {
        None
    };
    if let Some(last) = hierarchy.last_mut() {
        last.is_target = true;
        let skip_sibling = parent_class_fw.as_ref().map_or(false, |(cls, fw)| {
            fw == "Chrome" || cls.contains("Chrome_") || cls.contains("WebView")
        });
        if skip_sibling {
            log::info!("[PERF][CAP] Sibling index: skipped (parent is embedded browser: class='{}' fw='{}')",
                parent_class_fw.as_ref().map(|(c, _)| c.as_str()).unwrap_or(""),
                parent_class_fw.as_ref().map(|(_, f)| f.as_str()).unwrap_or(""));
        } else if let Ok(ctrl_walker) = auto.get_control_view_walker() {
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
    log::info!("[PERF][CAP] Sibling index: {}ms", t4.elapsed().as_millis());

    // 5. Set walker hints for faster XPath validation
    let t5 = std::time::Instant::now();
    let window_pid = hierarchy.get(window_index).map(|n| n.process_id).unwrap_or(0);
    set_walker_hints(&mut hierarchy, window_pid);

    // 5.5. Set included flag: all element nodes (after Window) default to included=true.
    // Users can manually uncheck nodes in UI to exclude them from XPath.
    // Window node (index 0) is always excluded from element XPath.
    for (i, node) in hierarchy.iter_mut().enumerate() {
        node.included = i != 0;
    }

    // Extract window_info BEFORE truncation
    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-boundary via child HWND enumeration
    let capture_mode = CaptureMode::Normal;
    let mut locate_mode = LocateMode::Fast;
    let mut child_hwnd_hint: Option<crate::core::model::ChildHwndHint> = None;
    if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
        info!("[Normal] Cross-boundary detected at index {} (window_pid={}), switching to FastChild mode",
            boundary_idx, window_pid);
        // Collect child HWND info before truncation
        if let Some(boundary_node) = hierarchy.get(boundary_idx) {
            child_hwnd_hint = Some(crate::core::model::ChildHwndHint {
                hwnd_class: boundary_node.class_name.clone(),
                hwnd_title: boundary_node.name.clone(),
            });
        }
        hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
        locate_mode = LocateMode::FastChild;
        for (i, n) in hierarchy.iter().enumerate() {
            info!("[TRUNCATED] idx={} name='{}' class='{}' ctrl='{}' depth={} included={}",
                i, n.name, n.class_name, n.control_type, n.depth_from_window, n.included);
        }
    }
    log::info!("[PERF][CAP] Cross-boundary + truncation: {}ms", t5.elapsed().as_millis());

    log::info!("[PERF][CAP] Capture total: {}ms (hierarchy_depth={} target='{}' mode={:?})",
        capture_start.elapsed().as_millis(), hierarchy.len(), target_name, capture_mode);

    let search_root = if locate_mode.is_child_mode() {
        if let Some(hint) = &child_hwnd_hint {
            crate::core::model::SearchRoot::ChildHwnd {
                class: hint.hwnd_class.clone(),
                title: hint.hwnd_title.clone(),
            }
        } else {
            crate::core::model::SearchRoot::Window
        }
    } else {
        crate::core::model::SearchRoot::Window
    };

    let search_context = SearchContext {
        locate_mode,
        child_hwnd_hint,
        search_root,
    };

    Ok(CaptureResult {
        hierarchy,
        cursor_x: x,
        cursor_y: y,
        error: None,
        window_info,
        capture_mode,
        locate_mode,
        search_context,
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

    // Keep the boundary node as a sub-window identifier in the hierarchy.
    // It acts as the first element node (depth=0) and its ClassName/Name
    // serve as child HWND constraints in XPath, speeding up validation.
    // Note: In child mode validation, the XPath first step (boundary node)
    // will be skipped because it's already matched via child_hwnd_hint.
    let mut boundary_node = hierarchy[boundary_idx].clone();
    boundary_node.depth_from_window = 0; // Root of the sub-HWND search scope
    let mut result = vec![window_node, boundary_node];
    for (i, mut node) in child_nodes.into_iter().enumerate() {
        node.depth_from_window = i + 1; // +1 because boundary node occupies depth=0
        result.push(node);
    }
    info!("[TRUNC_IN] result len={} (boundary node preserved as sub-window id)", result.len());
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
                capture_mode: CaptureMode::Enhanced,
                locate_mode: LocateMode::Full,
                search_context: SearchContext::default_full(),
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
                capture_mode: CaptureMode::Enhanced,
                locate_mode: LocateMode::Full,
                search_context: SearchContext::default_full(),
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
                None => { info!("[Enhanced] No walker available, skipping self-process fix"); return Ok(CaptureResult { hierarchy: vec![], cursor_x: x, cursor_y: y, error: Some("无法获取 TreeWalker".into()), window_info: None, capture_mode: CaptureMode::Enhanced, locate_mode: LocateMode::Full, search_context: SearchContext::default_full() }); }
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

    // Create BFS cache request for batch property prefetch (3-5x faster)
    let bfs_cache = create_bfs_cache_request(&auto);

    /// Inner BFS loop: walk the tree from `start_elem` level by level,
    /// returning the deepest element whose BoundingRectangle contains (x, y).
    /// Uses BuildCache for batch property prefetch when cache_request is available.
    fn bfs_find_deepest(
        walker: &UITreeWalker,
        start_elem: &UIElement,
        x: i32, y: i32,
        my_pid: u32,
        cache_request: &Option<UICacheRequest>,
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

            let mut child = match cache_request {
                Some(cr) => walker.get_first_child_build_cache(&current, cr).ok(),
                None => walker.get_first_child(&current).ok(),
            };
            while let Some(c) = child {
                // Skip elements from our own process
                let c_pid: u32 = if cache_request.is_some() {
                    c.get_cached_process_id().unwrap_or(0) as u32
                } else {
                    c.get_process_id().unwrap_or(0)
                };
                if c_pid as u32 == my_pid {
                    child = match cache_request {
                        Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                        None => walker.get_next_sibling(&c).ok(),
                    };
                    continue;
                }
                // Skip offscreen elements
                if let Ok(offscreen) = c.is_offscreen() {
                    if offscreen {
                        child = match cache_request {
                            Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                            None => walker.get_next_sibling(&c).ok(),
                        };
                        continue;
                    }
                }
                // Cycle detection (RuntimeId cannot be cached, always read from live)
                let rid = c.get_runtime_id().ok();
                if let Some(ref rid) = rid {
                    if visited_rids.contains(rid) {
                        child = match cache_request {
                            Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                            None => walker.get_next_sibling(&c).ok(),
                        };
                        continue;
                    }
                }
                // Check BoundingRectangle contains the point
                let rect = if cache_request.is_some() {
                    c.get_cached_bounding_rectangle()
                } else {
                    c.get_bounding_rectangle()
                };
                let rect = match rect {
                    Ok(r) => r,
                    Err(_) => {
                        child = match cache_request {
                            Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                            None => walker.get_next_sibling(&c).ok(),
                        };
                        continue;
                    }
                };
                let sr = SimpleRect::from(&rect);
                let w = sr.width();
                let h = sr.height();
                if w <= 0 || h <= 0 || !point_in_rect(x, y, &sr) {
                    child = match cache_request {
                        Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                        None => walker.get_next_sibling(&c).ok(),
                    };
                    continue;
                }

                let area = w as i64 * h as i64;
                let ct = if cache_request.is_some() {
                    c.get_cached_control_type().map(|ct| control_type_id_to_name(ct as i32).to_string()).unwrap_or_default()
                } else {
                    c.get_control_type_raw().map(control_type_name).unwrap_or_default()
                };
                let name = if cache_request.is_some() {
                    c.get_cached_name().unwrap_or_default()
                } else {
                    c.get_name().unwrap_or_default()
                };
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

                child = match cache_request {
                    Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                    None => walker.get_next_sibling(&c).ok(),
                };
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
    let mut target_elem = bfs_find_deepest(&raw_walker, &hit_elem, x, y, my_pid, &bfs_cache);

    // Step 2.1: Retry with pre-Step-1.5 element if BFS found nothing and cross-HWND changed
    if target_elem.is_none() && cross_hwnd_changed {
        info!("[Enhanced] BFS from cross-HWND element found no matches, retrying with pre-Step-1.5 element");
        target_elem = bfs_find_deepest(&raw_walker, &pre_cross_hwnd_elem, x, y, my_pid, &bfs_cache);
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
                            target_elem = bfs_find_deepest(&raw_walker, &child_elem, x, y, my_pid, &bfs_cache);
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
            let cache_request = create_hierarchy_cache_request(&auto);
            let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(ch.len());
            for (chain_idx, elem) in ch.iter().enumerate() {
                let node = if let Some(ref cr) = cache_request {
                    elem.build_updated_cache(cr).ok()
                        .and_then(|cached_elem| element_to_node_cached(&cached_elem))
                } else {
                    element_to_node(elem, &auto)
                };
                if let Some(mut node) = node {
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

            // Set included flag for fallback path (same as main path)
            for (i, node) in hierarchy.iter_mut().enumerate() {
                node.included = i != 0;
            }

            let window_info = extract_window_info(&hierarchy);
            info!("[Enhanced] Fallback hierarchy depth={}", hierarchy.len());

            // Cross-process detection for fallback path
            // (Same logic as main path — BFS may have failed because the element
            // is in a different process, e.g. Chrome WebView)
            let mut locate_mode = LocateMode::Full;
            let mut child_hwnd_hint: Option<crate::core::model::ChildHwndHint> = None;
            if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
                info!("[Enhanced] Fallback: cross-process detected at index {} (class='{}'), switching to FullChild mode",
                    boundary_idx, hierarchy.get(boundary_idx).map(|n| n.class_name.as_str()).unwrap_or("?"));
                if let Some(boundary_node) = hierarchy.get(boundary_idx) {
                    child_hwnd_hint = Some(crate::core::model::ChildHwndHint {
                        hwnd_class: boundary_node.class_name.clone(),
                        hwnd_title: boundary_node.name.clone(),
                    });
                }
                hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
                locate_mode = LocateMode::FullChild;
            }

            let empty = String::new();
            let empty2 = String::new();
            let target_ct = hierarchy.last().map(|n| &n.control_type).unwrap_or(&empty);
            let target_name = hierarchy.last().map(|n| &n.name).unwrap_or(&empty2);
            info!("[Enhanced] Fallback hierarchy: depth={} target='{}' name='{}' mode={:?}",
                hierarchy.len(), target_ct, target_name, locate_mode);

            let search_root = if locate_mode.is_child_mode() {
                if let Some(hint) = &child_hwnd_hint {
                    crate::core::model::SearchRoot::ChildHwnd {
                        class: hint.hwnd_class.clone(),
                        title: hint.hwnd_title.clone(),
                    }
                } else {
                    crate::core::model::SearchRoot::Window
                }
            } else {
                crate::core::model::SearchRoot::Window
            };

            let search_context = SearchContext {
                locate_mode,
                child_hwnd_hint,
                search_root,
            };

            return Ok(CaptureResult {
                hierarchy, cursor_x: x, cursor_y: y, error: None, window_info,
                capture_mode: CaptureMode::Enhanced, locate_mode,
                search_context,
            });
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
    let cache_request = create_hierarchy_cache_request(&auto);
    let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
    for (chain_idx, elem) in chain.iter().enumerate() {
        let node = if let Some(ref cr) = cache_request {
            elem.build_updated_cache(cr).ok()
                .and_then(|cached_elem| element_to_node_cached(&cached_elem))
        } else {
            element_to_node(elem, &auto)
        };
        if let Some(mut node) = node {
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

    // Set included flag: all element nodes (after Window) default to included=true.
    // Users can manually uncheck nodes in UI to exclude them from XPath.
    for (i, node) in hierarchy.iter_mut().enumerate() {
        node.included = i != 0;
    }

    let window_info = extract_window_info(&hierarchy);

    // 6. Detect cross-process boundary
    //
    // CORE FIX: Always check find_cross_boundary_index, not just when cross_hwnd_changed.
    //
    // Reason: ElementFromPoint may already return a Chrome WebView element directly
    // (without Step 1.5 changing it), so cross_hwnd_changed=false. But the hierarchy
    // still contains cross-process nodes (Chrome PID ≠ window PID), which must be
    // detected to set FullChild mode and generate [full-child] prefix.
    let capture_mode = CaptureMode::Enhanced;
    let mut locate_mode = LocateMode::Full;
    let mut child_hwnd_hint: Option<crate::core::model::ChildHwndHint> = None;
    if let Some(boundary_idx) = find_cross_boundary_index(&hierarchy, window_pid) {
        if cross_hwnd_changed {
            info!("[Enhanced] Cross-HWND + cross-process detected at index {}, switching to FullChild mode", boundary_idx);
        } else {
            info!("[Enhanced] Cross-process detected at index {} (cross_hwnd_changed=false, ElementFromPoint already returned cross-process element), switching to FullChild mode", boundary_idx);
        }
        // Collect child HWND info before truncation
        if let Some(boundary_node) = hierarchy.get(boundary_idx) {
            child_hwnd_hint = Some(crate::core::model::ChildHwndHint {
                hwnd_class: boundary_node.class_name.clone(),
                hwnd_title: boundary_node.name.clone(),
            });
        }
        hierarchy = truncate_hierarchy_to_child(hierarchy, boundary_idx, window_pid);
        locate_mode = LocateMode::FullChild;
    }
    let empty = String::new();
    let empty2 = String::new();
    let target_ct = hierarchy.last().map(|n| &n.control_type).unwrap_or(&empty);
    let target_name = hierarchy.last().map(|n| &n.name).unwrap_or(&empty2);
    let normal_ct = original_hit_elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
    let normal_name = original_hit_elem.get_name().unwrap_or_default();
    info!("[Enhanced] hierarchy depth={} target='{}' name='{}' mode={:?} | ElementFromPoint type='{}' name='{}'",
        hierarchy.len(), target_ct, target_name, locate_mode, normal_ct, normal_name);

    let search_root = if locate_mode.is_child_mode() {
        if let Some(hint) = &child_hwnd_hint {
            crate::core::model::SearchRoot::ChildHwnd {
                class: hint.hwnd_class.clone(),
                title: hint.hwnd_title.clone(),
            }
        } else {
            crate::core::model::SearchRoot::Window
        }
    } else {
        crate::core::model::SearchRoot::Window
    };

    let search_context = SearchContext {
        locate_mode,
        child_hwnd_hint,
        search_root,
    };

    Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None, window_info, capture_mode, locate_mode, search_context })
}

/// Light BFS to find the deepest leaf element at (x, y) from the hit element.
///
/// Normal capture's ElementFromPoint often returns a container element (Group, Pane)
/// instead of the actual interactive target. This function walks down the ControlView
/// tree to find the smallest element containing (x, y).
///
/// Parameters:
/// - `auto`: UIAutomation instance
/// - `start_elem`: The element returned by ElementFromPoint
/// - `x`, `y`: Cursor coordinates
/// - `max_depth`: Maximum BFS depth (default 5)
///
/// Returns: The deepest leaf element, or None if no deeper element found.
fn light_bfs_to_leaf(
    auto: &UIAutomation,
    start_elem: &UIElement,
    x: i32,
    y: i32,
    max_depth: u32,
) -> Option<UIElement> {
    let walker = auto.get_control_view_walker().ok()?;
    let cache_request = create_bfs_cache_request(auto);
    let mut current = start_elem.clone();
    let mut visited_rids: HashSet<Vec<i32>> = HashSet::new();
    if let Ok(rid) = current.get_runtime_id() { visited_rids.insert(rid); }

    for _depth in 0..max_depth {
        let mut best_child: Option<UIElement> = None;
        let mut best_area = i64::MAX;
        let mut best_is_leaf = false;

        // Use BuildCache walker API to get children with prefetched properties
        let mut child = match &cache_request {
            Some(cr) => walker.get_first_child_build_cache(&current, cr).ok(),
            None => walker.get_first_child(&current).ok(),
        };
        while let Some(c) = child {
            // Cycle detection (RuntimeId cannot be cached, always read from live)
            let rid = c.get_runtime_id().ok();
            if let Some(ref rid) = rid {
                if visited_rids.contains(rid) {
                    child = match &cache_request {
                        Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                        None => walker.get_next_sibling(&c).ok(),
                    };
                    continue;
                }
            }
            // Skip offscreen elements (always from live)
            if c.is_offscreen().unwrap_or(false) {
                child = match &cache_request {
                    Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                    None => walker.get_next_sibling(&c).ok(),
                };
                continue;
            }
            // Check BoundingRectangle contains the point
            let rect = if cache_request.is_some() {
                c.get_cached_bounding_rectangle()
            } else {
                c.get_bounding_rectangle()
            };
            let rect = match rect {
                Ok(r) => r,
                Err(_) => {
                    child = match &cache_request {
                        Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                        None => walker.get_next_sibling(&c).ok(),
                    };
                    continue;
                }
            };
            let sr = SimpleRect::from(&rect);
            let w = sr.width();
            let h = sr.height();
            if w <= 0 || h <= 0 || !point_in_rect(x, y, &sr) {
                child = match &cache_request {
                    Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                    None => walker.get_next_sibling(&c).ok(),
                };
                continue;
            }

            let area = w as i64 * h as i64;
            let ct = if cache_request.is_some() {
                c.get_cached_control_type().map(|ct| control_type_id_to_name(ct as i32).to_string()).unwrap_or_default()
            } else {
                c.get_control_type_raw().map(control_type_name).unwrap_or_default()
            };
            let c_is_leaf = is_leaf_control_type(&ct);

            // Prefer leaf controls (Button, Edit, Text, etc.) over containers,
            // and among same category, prefer smaller area (more specific)
            let should_replace = if c_is_leaf && !best_is_leaf {
                true
            } else if !c_is_leaf && best_is_leaf {
                false
            } else {
                area < best_area
            };

            if should_replace {
                best_child = Some(c.clone());
                best_area = area;
                best_is_leaf = c_is_leaf;
            }

            child = match &cache_request {
                Some(cr) => walker.get_next_sibling_build_cache(&c, cr).ok(),
                None => walker.get_next_sibling(&c).ok(),
            };
        }

        match best_child {
            Some(c) => {
                if let Ok(rid) = c.get_runtime_id() { visited_rids.insert(rid); }
                current = c;
            }
            None => break, // No deeper child found
        }
    }

    // Return the found element only if it's different from the start
    if auto.compare_elements(&current, start_elem).unwrap_or(false) {
        None
    } else {
        Some(current)
    }
}

/// Two-phase highlight query result.
///
/// Phase 1: ElementFromPoint returns immediately — used for instant highlight.
/// Phase 2: BFS refines to deeper leaf — updates highlight if a better element is found.
pub enum HighlightPhase {
    /// Phase 1: ElementFromPoint direct hit — displayed instantly.
    Immediate {
        x: i32,
        y: i32,
        rect: ElementRect,
        control_type: String,
        name: String,
        class_name: String,
    },
    /// Phase 2: BFS found a deeper/better element at the point.
    Refined {
        x: i32,
        y: i32,
        rect: ElementRect,
        control_type: String,
        name: String,
        class_name: String,
    },
}

/// Extract rect and properties from a UIA element into highlight data.
fn elem_to_highlight_data(
    elem: &UIElement,
    x: i32,
    y: i32,
) -> Option<(i32, i32, ElementRect, String, String, String)> {
    let rect = elem.get_bounding_rectangle().ok()?;
    let ct = elem.get_control_type_raw()
        .map(control_type_name)
        .unwrap_or_default();
    let name = elem.get_name().unwrap_or_default();
    let class = elem.get_classname().unwrap_or_default();
    Some((
        x, y,
        ElementRect {
            x: rect.get_left(),
            y: rect.get_top(),
            width: rect.get_width(),
            height: rect.get_height(),
        },
        ct, name, class,
    ))
}

/// Lightweight highlight query — ElementFromPoint first (instant), then BFS refine.
///
/// This is used during capture hover. The strategy:
/// 1. ElementFromPoint returns a ControlView element — display highlight immediately.
/// 2. BFS (max depth 2) searches for a deeper leaf — if found, update highlight.
///
/// Uses a channel-based two-phase approach: Immediate sent first, Refined sent later
/// (only if BFS found a different element).
pub fn highlight_query(tx: std::sync::mpsc::Sender<HighlightPhase>, x: i32, y: i32) {
    let auto = match get_automation() {
        Ok(a) => a,
        Err(e) => {
            log::error!("[Highlight] get_automation failed: {e}");
            return;
        }
    };

    let point = UiaPoint::new(x, y);
    let hit_elem = match auto.element_from_point(point) {
        Ok(e) => e,
        Err(e) => {
            log::error!("[Highlight] ElementFromPoint failed: {e}");
            return;
        }
    };

    // Phase 1: Send ElementFromPoint result immediately for instant highlight
    if let Some(data) = elem_to_highlight_data(&hit_elem, x, y) {
        let (x, y, rect, ct, name, class) = data;
        if tx.send(HighlightPhase::Immediate {
            x, y, rect, control_type: ct, name, class_name: class,
        }).is_err() {
            return; // Receiver dropped, no point continuing
        }
    }

    // Phase 2: BFS to find deeper leaf, send Refined if different
    if let Some(refined) = light_bfs_to_leaf(&auto, &hit_elem, x, y, 2) {
        if !auto.compare_elements(&refined, &hit_elem).unwrap_or(true) {
            if let Some(data) = elem_to_highlight_data(&refined, x, y) {
                let (x, y, rect, ct, name, class) = data;
                let _ = tx.send(HighlightPhase::Refined {
                    x, y, rect, control_type: ct, name, class_name: class,
                });
            }
        }
    }
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
