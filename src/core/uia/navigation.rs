use super::*;

pub fn navigate_from_element(
    window_selector: &str,
    base_xpath: &str,
    steps: &[crate::core::model::NavigateStep],
) -> Result<(Option<crate::core::model::ElementData>, String), String> {
    use crate::core::model::NavigateStep;

    let auto = get_automation().map_err(|e| format!("UIAutomation init failed: {}", e))?;
    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return Err(format!("Window not found: {}", window_selector));
    }

    // Phase 1: Find base element (one XPath search)
    let mut base_elem: Option<UIElement> = None;
    let mut window_rect: Option<crate::core::model::Rect> = None;

    for window in &windows {
        let wr = window.get_bounding_rectangle().ok().map(|r| {
            crate::core::model::Rect { x: r.get_left(), y: r.get_top(), width: r.get_right() - r.get_left(), height: r.get_bottom() - r.get_top() }
        });
        if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window, base_xpath, 5000) {
            if let Some(elem) = elements.into_iter().next() {
                base_elem = Some(elem);
                window_rect = wr;
                break;
            }
        }
    }

    let mut current = base_elem.ok_or_else(|| format!("Base element not found: {}", base_xpath))?;
    log::info!("[Compass] Base element found, applying {} steps", steps.len());

    // Phase 2: Walk tree step-by-step using TreeWalker
    // Prefer RawViewWalker for Chrome/WebView compatibility,
    // fall back to ControlViewWalker if RawViewWalker fails.
    let raw_walker = auto.get_raw_view_walker().ok();
    let ctrl_walker = auto.get_control_view_walker().ok();
    let walker = raw_walker.as_ref().or(ctrl_walker.as_ref())
        .ok_or_else(|| "Failed to create TreeWalker".to_string())?;

    for (i, step) in steps.iter().enumerate() {
        let before_label = format!("step[{}]={:?}", i, step);
        match step {
            NavigateStep::Parent { levels } => {
                for lv in 0..*levels {
                    match walker.get_parent(&current) {
                        Ok(parent) => {
                            // Check if the parent is the desktop root (stop)
                            let desktop = auto.get_root_element().ok();
                            let is_desktop = desktop.as_ref().map_or(false, |d| {
                                auto.compare_elements(&parent, d).unwrap_or(false)
                            });
                            if is_desktop {
                                return Err(format!("Step {} parent({}): reached Desktop root", i, lv + 1));
                            }
                            current = parent;
                        }
                        Err(e) => {
                            return Err(format!("Step {} parent({}): GetParent failed: {:?}", i, lv + 1, e));
                        }
                    }
                }
                log::info!("[Compass] {}: parent({}) OK", before_label, levels);
            }

            NavigateStep::Child { index } => {
                let parent = current.clone();
                // Enumerate all direct children
                let mut children: Vec<UIElement> = Vec::new();
                let mut child = walker.get_first_child(&parent).ok();
                while let Some(c) = child {
                    children.push(c.clone());
                    child = walker.get_next_sibling(&c).ok();
                }

                let child_count = children.len() as i32;
                let resolved = if *index >= 0 {
                    *index
                } else {
                    // Negative index: -1 = last, -2 = second-to-last
                    child_count + *index
                };

                if resolved < 0 || resolved >= child_count {
                    return Err(format!(
                        "Step {} child({}): index out of range (resolved={}, count={})",
                        i, index, resolved, child_count
                    ));
                }
                current = children[resolved as usize].clone();
                log::info!("[Compass] {}: child({}) OK (resolved={}, total={})", before_label, index, resolved, child_count);
            }

            NavigateStep::SiblingAbs { index } => {
                // sibling_abs(N) = parent().child(N)
                let parent = match walker.get_parent(&current) {
                    Ok(p) => p,
                    Err(e) => return Err(format!("Step {} sibling_abs({}): GetParent failed: {:?}", i, index, e)),
                };
                let mut siblings: Vec<UIElement> = Vec::new();
                let mut child = walker.get_first_child(&parent).ok();
                while let Some(c) = child {
                    siblings.push(c.clone());
                    child = walker.get_next_sibling(&c).ok();
                }
                let sib_count = siblings.len() as i32;
                let resolved = if *index >= 0 { *index } else { sib_count + *index };
                if resolved < 0 || resolved >= sib_count {
                    return Err(format!(
                        "Step {} sibling_abs({}): index out of range (resolved={}, count={})",
                        i, index, resolved, sib_count
                    ));
                }
                current = siblings[resolved as usize].clone();
                log::info!("[Compass] {}: sibling_abs({}) OK", before_label, index);
            }

            NavigateStep::SiblingLeft { offset } => {
                // preceding-sibling: go left by offset
                for off in 1..=*offset {
                    match walker.get_previous_sibling(&current) {
                        Ok(prev) => current = prev,
                        Err(e) => return Err(format!(
                            "Step {} sibling_left({}): GetPreviousSibling at offset {} failed: {:?}",
                            i, offset, off, e
                        )),
                    }
                }
                log::info!("[Compass] {}: sibling_left({}) OK", before_label, offset);
            }

            NavigateStep::SiblingRight { offset } => {
                // following-sibling: go right by offset
                if *offset == 0 {
                    log::warn!("[Compass] Step {}: sibling_right(0) is a no-op", i);
                }
                for off in 1..=*offset {
                    match walker.get_next_sibling(&current) {
                        Ok(next) => current = next,
                        Err(e) => return Err(format!(
                            "Step {} sibling_right({}): GetNextSibling at offset {} failed: {:?}",
                            i, offset, off, e
                        )),
                    }
                }
                log::info!("[Compass] {}: sibling_right({}) OK", before_label, offset);
            }
        }
    }

    // Phase 3: Convert result to ElementInfo
    let mut rng = rand::thread_rng();
    let info = element_info_from_uia(&current, window_rect.as_ref(), 5.0, &mut rng);

    let find_selector = String::new();

    Ok((info, find_selector))
}

pub fn find_visible_elements(
    window_selector: &str,
    container_xpath: &str,
    control_types: &[&str],
) -> Vec<crate::core::model::ElementData> {
    use crate::core::model::{Rect, ElementData};

    let auto = match get_automation() {
        Ok(a) => a,
        Err(_) => return vec![],
    };

    // 1. 定位容器元素
    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        log::warn!("[find_visible_elements] No window found for selector: {}", window_selector);
        return vec![];
    }

    let mut container_elem: Option<UIElement> = None;
    for win in &windows {
        if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, win, container_xpath, 5000) {
            if let Some(first) = elements.first() {
                container_elem = Some(first.clone());
                break;
            }
        }
    }

    let container = match container_elem {
        Some(e) => e,
        None => {
            log::warn!("[find_visible_elements] Container not found: {}", container_xpath);
            return vec![];
        }
    };

    // 2. 构建 IsOffscreen = FALSE 条件
    let offscreen_false = match auto.create_property_condition(
        UIProperty::IsOffscreen,
        Variant::from(false),
        None,
    ) {
        Ok(c) => c,
        Err(e) => {
            log::error!("[find_visible_elements] CreatePropertyCondition(IsOffscreen=false) failed: {}", e);
            return vec![];
        }
    };

    // 3. 构建 ControlType 条件（如果指定了 control_types）
    let condition = if control_types.is_empty() {
        // 不按 ControlType 过滤，只用 IsOffscreen=false
        offscreen_false
    } else if control_types.len() == 1 {
        // 单个 ControlType → 直接与 IsOffscreen AND
        let ct_id = match control_type_name_to_id(control_types[0]) {
            Some(id) => id,
            None => {
                log::warn!("[find_visible_elements] Unknown control type: {}", control_types[0]);
                return vec![];
            }
        };

        let ct_condition = match auto.create_property_condition(
            UIProperty::ControlType,
            Variant::from(ct_id),
            None,
        ) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[find_visible_elements] CreatePropertyCondition(ControlType) failed: {}", e);
                return vec![];
            }
        };

        match auto.create_and_condition(offscreen_false, ct_condition) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[find_visible_elements] CreateAndCondition failed: {}", e);
                // Fallback: return offscreen_false (already consumed by create_and_condition)
                match auto.create_property_condition(UIProperty::IsOffscreen, Variant::from(false), None) {
                    Ok(c) => c,
                    Err(_) => return vec![],
                }
            }
        }
    } else {
        // 多个 ControlType → 先用 OrCondition 合并，再与 IsOffscreen AND
        let mut ct_conditions: Vec<UICondition> = Vec::new();
        for ct_name in control_types {
            if let Some(ct_id) = control_type_name_to_id(ct_name) {
                if let Ok(cond) = auto.create_property_condition(
                    UIProperty::ControlType,
                    Variant::from(ct_id),
                    None,
                ) {
                    ct_conditions.push(cond);
                }
            }
        }

        if ct_conditions.is_empty() {
            match auto.create_property_condition(UIProperty::IsOffscreen, Variant::from(false), None) {
                Ok(c) => c,
                Err(_) => return vec![],
            }
        } else if ct_conditions.len() == 1 {
            let ct_cond = ct_conditions.remove(0);
            // Need a fresh offscreen condition since the original was consumed
            let offscreen_cond = match auto.create_property_condition(UIProperty::IsOffscreen, Variant::from(false), None) {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            match auto.create_and_condition(offscreen_cond.clone(), ct_cond) {
                Ok(c) => c,
                Err(_) => offscreen_cond, // best effort
            }
        } else {
            // OrCondition: chain conditions with create_or_condition
            let mut ct_or: Option<UICondition> = Some(ct_conditions.remove(0));
            for cond in ct_conditions {
                let current = match ct_or.take() {
                    Some(c) => c,
                    None => break,
                };
                match auto.create_or_condition(current, cond) {
                    Ok(c) => ct_or = Some(c),
                    Err(e) => {
                        log::error!("[find_visible_elements] CreateOrCondition failed: {}", e);
                        return vec![];
                    }
                }
            }
            let ct_or = ct_or.unwrap_or_else(|| {
                auto.create_true_condition().expect("CreateTrueCondition failed")
            });
            // Fresh offscreen condition
            let offscreen_cond = match auto.create_property_condition(UIProperty::IsOffscreen, Variant::from(false), None) {
                Ok(c) => c,
                Err(_) => return vec![],
            };
            match auto.create_and_condition(offscreen_cond.clone(), ct_or) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreateAndCondition(Offscreen, CT_OR) failed: {}", e);
                    offscreen_cond
                }
            }
        }
    };

    // 4. FindAll(TreeScope::Descendants) 执行查询
    let raw_elements = match container.find_all(TreeScope::Descendants, &condition) {
        Ok(arr) => arr,
        Err(e) => {
            log::error!("[find_visible_elements] FindAll failed: {}", e);
            return vec![];
        }
    };

    log::info!("[find_visible_elements] Found {} visible elements (types={:?})", raw_elements.len(), control_types);

    // 获取容器矩形用于计算 visibleRect
    let container_rect = container.get_bounding_rectangle().ok().map(|r| {
        Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }
    });

    // 5. 转换为 ElementInfo
    raw_elements.iter().filter_map(|elem| {
        let r = match elem.get_bounding_rectangle() {
            Ok(r) => r,
            Err(_) => return None,
        };
        let api_rect = Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        };
        let center = api_rect.center();
        
        // 计算 visibleRect
        let visible_rect = compute_element_visible_rect(&api_rect, container_rect.as_ref());
        let is_offscreen = elem.is_offscreen().unwrap_or(false);
        let (center_opt, cr_opt) = if is_offscreen {
            (None, None)
        } else {
            (Some(center), Some(center))
        };

        Some(ElementData {
            rect: Some(api_rect),
            visible_rect,
            center: center_opt,
            center_random: cr_opt,
            control_type: elem.get_control_type_raw().map(control_type_name).unwrap_or_default(),
            name: elem.get_name().unwrap_or_default(),
            automation_id: elem.get_automation_id().unwrap_or_default(),
            class_name: elem.get_classname().unwrap_or_default(),
            framework_id: elem.get_framework_id().unwrap_or_default(),
            help_text: elem.get_help_text().unwrap_or_default(),
            localized_control_type: elem.get_localized_control_type().unwrap_or_default(),
            is_enabled: elem.is_enabled().unwrap_or(true),
            is_offscreen,
            is_password: elem.is_password().unwrap_or(false),
            accelerator_key: elem.get_accelerator_key().unwrap_or_default(),
            access_key: elem.get_access_key().unwrap_or_default(),
            item_type: elem.get_item_type().unwrap_or_default(),
            item_status: elem.get_item_status().unwrap_or_default(),
            process_id: elem.get_process_id().unwrap_or(0),
            runtime_id: runtime_id_key(&elem).map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
            is_checkable: None,
            is_checked: None,
            is_clickable: None,
            is_scrollable: None,
            is_selected: None,
        })
    }).collect()
}
