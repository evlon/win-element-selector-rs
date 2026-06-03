use super::*;

pub fn navigate_from_element(
    window_selector: &str,
    base_xpath: &str,
    steps: &[crate::api::types::NavigateStep],
) -> Result<(Option<crate::api::types::ElementInfo>, String), String> {
    use crate::api::types::NavigateStep;

    let auto = get_automation().map_err(|e| format!("IUIAutomation init failed: {}", e))?;
    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return Err(format!("Window not found: {}", window_selector));
    }

    // 窗口 XPath 前缀，用于构造 findSelector
    let _window_prefix = window_selector.to_string();

    // Phase 1: Find base element (one XPath search)
    let mut base_elem: Option<IUIAutomationElement> = None;
    let mut window_rect: Option<crate::api::types::Rect> = None;

    for window in &windows {
        let wr = unsafe {
            window.CurrentBoundingRectangle().ok().map(|r| {
                crate::api::types::Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
            })
        };
        if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window, base_xpath) {
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
    let raw_walker = unsafe { auto.RawViewWalker().ok() };
    let ctrl_walker = unsafe { auto.ControlViewWalker().ok() };
    let walker = raw_walker.as_ref().or(ctrl_walker.as_ref())
        .ok_or_else(|| "Failed to create TreeWalker".to_string())?;

    for (i, step) in steps.iter().enumerate() {
        let before_label = format!("step[{}]={:?}", i, step);
        match step {
            NavigateStep::Parent { levels } => {
                for lv in 0..*levels {
                    match unsafe { walker.GetParentElement(&current) } {
                        Ok(parent) => {
                            // Check if the parent is the desktop root (stop)
                            let desktop = unsafe { auto.GetRootElement().ok() };
                            let is_desktop = desktop.as_ref().map_or(false, |d| {
                                unsafe { auto.CompareElements(&parent, d).unwrap_or(windows::core::BOOL(0)).as_bool() }
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
                let mut children: Vec<IUIAutomationElement> = Vec::new();
                let mut child = unsafe { walker.GetFirstChildElement(&parent).ok() };
                while let Some(c) = child {
                    children.push(c.clone());
                    child = unsafe { walker.GetNextSiblingElement(&c).ok() };
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
                let parent = match unsafe { walker.GetParentElement(&current) } {
                    Ok(p) => p,
                    Err(e) => return Err(format!("Step {} sibling_abs({}): GetParent failed: {:?}", i, index, e)),
                };
                let mut siblings: Vec<IUIAutomationElement> = Vec::new();
                let mut child = unsafe { walker.GetFirstChildElement(&parent).ok() };
                while let Some(c) = child {
                    siblings.push(c.clone());
                    child = unsafe { walker.GetNextSiblingElement(&c).ok() };
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
                    match unsafe { walker.GetPreviousSiblingElement(&current) } {
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
                    match unsafe { walker.GetNextSiblingElement(&current) } {
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

    // findSelector 由 SDK 本地构造（buildCompassXpath 等），后端不再从属性构建，
    // 因为属性构建的 XPath 无法定位 Chrome 内嵌元素等场景
    let find_selector = String::new();

    Ok((info, find_selector))
}

pub fn find_visible_elements(
    window_selector: &str,
    container_xpath: &str,
    control_types: &[&str],
) -> Vec<crate::api::types::ElementInfo> {
    use crate::api::types::{ElementInfo, Rect};
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Variant::*;

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

    let mut container_elem: Option<IUIAutomationElement> = None;
    for win in &windows {
        if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, win, container_xpath) {
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
    let offscreen_false = {
        let mut variant = VARIANT::default();
        unsafe {
            let var_ptr = &mut variant as *mut VARIANT;
            // VT_BOOL = 11
            let vt_ptr = var_ptr as *mut VARENUM;
            std::ptr::write(vt_ptr, VT_BOOL);
            // VARIANT_BOOL: VARIANT_TRUE = -1 (0xFFFF), VARIANT_FALSE = 0
            // The boolVal field is at offset 8 in the union (same as bstrVal)
            let bool_ptr = (var_ptr as *mut u8).add(8) as *mut i16;
            std::ptr::write(bool_ptr, 0); // VARIANT_FALSE = IsOffscreen = false
        }
        match unsafe { auto.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &variant) } {
            Ok(c) => c,
            Err(e) => {
                log::error!("[find_visible_elements] CreatePropertyCondition(IsOffscreen=false) failed: {}", e);
                return vec![];
            }
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

        let ct_condition = {
            let mut variant = VARIANT::default();
            unsafe {
                let var_ptr = &mut variant as *mut VARIANT;
                let vt_ptr = var_ptr as *mut VARENUM;
                std::ptr::write(vt_ptr, VT_I4);
                let i4_ptr = (var_ptr as *mut u8).add(8) as *mut i32;
                std::ptr::write(i4_ptr, ct_id);
            }
            match unsafe { auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant) } {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreatePropertyCondition(ControlType) failed: {}", e);
                    return vec![];
                }
            }
        };

        match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_condition) } {
            Ok(c) => c,
            Err(e) => {
                log::error!("[find_visible_elements] CreateAndCondition failed: {}", e);
                offscreen_false // fallback: 只用 offscreen 条件
            }
        }
    } else {
        // 多个 ControlType → 先用 OrCondition 合并，再与 IsOffscreen AND
        let ct_conditions: Vec<Option<IUIAutomationCondition>> = control_types.iter()
            .filter_map(|ct_name| {
                let ct_id = control_type_name_to_id(ct_name)?;
                let mut variant = VARIANT::default();
                unsafe {
                    let var_ptr = &mut variant as *mut VARIANT;
                    let vt_ptr = var_ptr as *mut VARENUM;
                    std::ptr::write(vt_ptr, VT_I4);
                    let i4_ptr = (var_ptr as *mut u8).add(8) as *mut i32;
                    std::ptr::write(i4_ptr, ct_id);
                }
                unsafe { auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant) }.ok()
            })
            .map(Some)
            .collect();

        if ct_conditions.is_empty() {
            offscreen_false
        } else if ct_conditions.len() == 1 {
            let ct_cond = ct_conditions[0].clone().unwrap();
            match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_cond) } {
                Ok(c) => c,
                Err(_) => offscreen_false,
            }
        } else {
            // OrCondition 合并多个 ControlType
            let ct_or = match unsafe { auto.CreateOrConditionFromNativeArray(&ct_conditions) } {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreateOrCondition failed: {}", e);
                    return vec![];
                }
            };
            match unsafe { auto.CreateAndCondition(&offscreen_false, &ct_or) } {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[find_visible_elements] CreateAndCondition(Offscreen, CT_OR) failed: {}", e);
                    offscreen_false
                }
            }
        }
    };

    // 4. FindAll(TreeScope_Descendants) 执行查询
    let raw_elements = match unsafe { container.FindAll(TreeScope_Descendants, &condition) } {
        Ok(arr) => arr,
        Err(e) => {
            log::error!("[find_visible_elements] FindAll failed: {}", e);
            return vec![];
        }
    };

    let count = match unsafe { raw_elements.Length() } {
        Ok(c) => c,
        Err(_) => 0,
    };

    log::info!("[find_visible_elements] Found {} visible elements (types={:?})", count, control_types);

    // 获取容器矩形用于计算 visibleRect
    let container_rect = unsafe {
        container.CurrentBoundingRectangle().ok().map(|r| {
            Rect {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
            }
        })
    };

    // 5. 转换为 ElementInfo
    (0..count).filter_map(|i| {
        let elem = match unsafe { raw_elements.GetElement(i) } {
            Ok(e) => e,
            Err(_) => return None,
        };

        let r = match unsafe { elem.CurrentBoundingRectangle() } {
            Ok(r) => r,
            Err(_) => return None,
        };
        let api_rect = Rect {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
        };
        let center = api_rect.center();
        
        // 计算 visibleRect
        let visible_rect = compute_element_visible_rect(&api_rect, container_rect.as_ref());
        let is_offscreen = unsafe { elem.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) };
        let (center_opt, cr_opt) = if is_offscreen {
            (None, None)
        } else {
            (Some(center), Some(center)) // 不需要随机偏移，center_random = center
        };

        Some(ElementInfo {
            rect: Some(api_rect),
            visible_rect,
            center: center_opt,
            center_random: cr_opt,
            control_type: unsafe { elem.CurrentControlType().map(control_type_name).unwrap_or_default() },
            name: get_bstr(unsafe { elem.CurrentName() }),
            automation_id: get_bstr(unsafe { elem.CurrentAutomationId() }),
            class_name: get_bstr(unsafe { elem.CurrentClassName() }),
            framework_id: get_bstr(unsafe { elem.CurrentFrameworkId() }),
            help_text: get_bstr(unsafe { elem.CurrentHelpText() }),
            localized_control_type: get_bstr(unsafe { elem.CurrentLocalizedControlType() }),
            is_enabled: unsafe { elem.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true) },
            is_offscreen,
            is_password: unsafe { elem.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) },
            accelerator_key: get_bstr(unsafe { elem.CurrentAcceleratorKey() }),
            access_key: get_bstr(unsafe { elem.CurrentAccessKey() }),
            item_type: get_bstr(unsafe { elem.CurrentItemType() }),
            item_status: get_bstr(unsafe { elem.CurrentItemStatus() }),
            process_id: unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 },
            runtime_id: runtime_id_key(&elem).map(|ids| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
            is_checkable: None,
            is_checked: None,
            is_clickable: None,
            is_scrollable: None,
            is_selected: None,
        })
    }).collect()
}

