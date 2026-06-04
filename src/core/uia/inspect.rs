use super::*;

pub fn extract_children_features(
    automation: &UIAutomation,
    element: &UIElement,
    parent_rect: &RECT,
) -> Vec<crate::core::model::ChildFeature> {
    use crate::core::model::{ChildFeature, RelativeRect};
    
    let mut features = vec![];
    
    // 创建条件：获取所有子元素
    let condition = match automation.create_true_condition() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    
    // 查找所有直接子元素
    let children = match element.find_all(TreeScope::Children, &condition) {
        Ok(arr) => arr,
        Err(_) => return vec![],
    };
    
    let parent_width = (parent_rect.right - parent_rect.left) as f32;
    let parent_height = (parent_rect.bottom - parent_rect.top) as f32;
    
    if parent_width <= 0.0 || parent_height <= 0.0 {
        return vec![];
    }
    
    for child in &children {
        // 获取子元素的 ControlType ID
        if let Ok(control_type_id) = child.get_control_type_raw() {
            let control_type = control_type_name(control_type_id);
            
            // 获取子元素的边界
            if let Ok(child_rect) = child.get_bounding_rectangle() {
                let child_width = (child_rect.get_right() - child_rect.get_left()) as f32;
                let child_height = (child_rect.get_bottom() - child_rect.get_top()) as f32;
                
                // 计算相对于父元素的归一化坐标
                let x_ratio = (child_rect.get_left() - parent_rect.left) as f32 / parent_width;
                let y_ratio = (child_rect.get_top() - parent_rect.top) as f32 / parent_height;
                let width_ratio = child_width / parent_width;
                let height_ratio = child_height / parent_height;
                
                // 限制在 [0, 1] 范围内
                let x_ratio = x_ratio.clamp(0.0, 1.0);
                let y_ratio = y_ratio.clamp(0.0, 1.0);
                let width_ratio = width_ratio.clamp(0.0, 1.0);
                let height_ratio = height_ratio.clamp(0.0, 1.0);
                
                features.push(ChildFeature {
                    control_type,
                    relative_bounds: RelativeRect {
                        x_ratio,
                        y_ratio,
                        width_ratio,
                        height_ratio,
                    },
                });
            }
        }
    }
    
    features
}

/// Inspect 返回的单个节点信息（核心模型）
#[derive(Debug, Clone, serde::Serialize)]
pub struct InspectNode {
    /// 元素层级深度（根元素为 0）
    pub depth: usize,
    /// 控件类型，如 "Button"、"Text"、"Edit" 等
    pub control_type: String,
    /// 控件的 Name 属性
    pub name: String,
    /// 控件的 ClassName 属性
    pub class_name: String,
    /// 控件的 AutomationId 属性
    pub automation_id: String,
    /// 控件的 FrameworkId 属性
    pub framework_id: String,
    /// 控件的文本内容（通过 ValuePattern 获取）
    pub text_value: Option<String>,
    /// 控件的 HelpText 属性（辅助说明文字）
    pub help_text: String,
    /// 控件的 ItemType 属性
    pub item_type: String,
    /// 控件的 ItemStatus 属性
    pub item_status: String,
    /// 控件的区域位置
    pub rect: Option<crate::core::model::Rect>,
    /// 是否在屏幕外
    pub is_offscreen: bool,
    /// 选中该控件相对于根元素的 XPath 表达式
    pub relative_xpath: String,
    /// 子节点列表
    pub children: Vec<InspectNode>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InspectResult {
    /// 是否成功
    pub success: bool,
    /// 根元素 XPath
    pub root_xpath: String,
    /// 结构化节点树
    pub nodes: Option<InspectNode>,
    /// 扁平化节点列表（DFS 顺序，无嵌套 children）
    pub flat_nodes: Vec<InspectNode>,
    /// 格式化文本（format='txt' 时有值）
    pub text_output: Option<String>,
    /// 子元素总数
    pub total_children: usize,
    /// 错误信息
    pub error: Option<String>,
}

pub fn inspect_subtree(
    window_selector: &str,
    element_xpath: &str,
    max_depth: usize,
    max_nodes: usize,
    format: &str,
) -> InspectResult {
    use std::time::Instant;
    let start = Instant::now();

    let auto = match get_automation() {
        Ok(a) => a,
        Err(e) => {
            return InspectResult {
                success: false,
                root_xpath: element_xpath.to_string(),
                nodes: None,
                flat_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("获取 UIAutomation 实例失败: {}", e)),
            };
        }
    };

    // Step 1: 查找目标窗口
    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return InspectResult {
            success: false,
            root_xpath: element_xpath.to_string(),
            nodes: None,
            flat_nodes: vec![],
            text_output: None,
            total_children: 0,
            error: Some(format!("窗口未找到: {}", window_selector)),
        };
    }

    // Step 2: 在窗口中查找目标元素
    let (locate_mode, _, _stripped) = LocateMode::strip_xpath_prefix(element_xpath);
    let is_child_mode = locate_mode.map_or(false, |m| m.is_child_mode());

    let mut target_element: Option<UIElement> = None;

    if is_child_mode {
        log::info!("[inspect_subtree] Child mode detected, searching via EnumChildWindows");
        for window in &windows {
            let hwnd = match window.get_native_window_handle() {
                Ok(h) => {
                    let raw: windows::Win32::Foundation::HANDLE = h.into();
                    HWND(raw.0)
                }
                Err(_) => continue,
            };
            let child_hwnds = enum_child_hwnds(hwnd);
            log::info!("[inspect_subtree] Child mode: {} child HWNDs for window", child_hwnds.len());

            for child_hwnd in &child_hwnds {
                let child_elem = match auto.element_from_handle((*child_hwnd).into()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                // Child HWND filtering via prefix attributes; XPath used as-is
                let child_xpath = element_xpath;
                if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, &child_elem, &child_xpath) {
                    if let Some(elem) = elements.into_iter().next() {
                        target_element = Some(elem);
                        break;
                    }
                }
            }
            if target_element.is_some() {
                break;
            }
        }
    } else {
        for window in &windows {
            if let Ok((elements, _)) = find_by_xpath_with_fallback(&auto, window, element_xpath) {
                if let Some(elem) = elements.into_iter().next() {
                    target_element = Some(elem);
                    break;
                }
            }
        }
    }

    let root_element = match target_element {
        Some(e) => e,
        None => {
            return InspectResult {
                success: false,
                root_xpath: element_xpath.to_string(),
                nodes: None,
                flat_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("元素未找到: {}", element_xpath)),
            };
        }
    };

    // Step 3: 使用 RawViewWalker 递归遍历子树
    let raw_walker = match auto.get_raw_view_walker() {
        Ok(w) => w,
        Err(e) => {
            return InspectResult {
                success: false,
                root_xpath: element_xpath.to_string(),
                nodes: None,
                flat_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("获取 RawViewWalker 失败: {}", e)),
            };
        }
    };

    // 计数器，用于限制总节点数和跟踪同类型兄弟索引
    let mut total_count = 0usize;

    // DFS 遍历构建节点树
    let root_node = build_inspect_node(
        &root_element,
        &raw_walker,
        0,       // depth
        max_depth,
        max_nodes,
        &mut total_count,
        "",
    );

    log::info!(
        "[inspect_subtree] Completed in {}ms, total_nodes={}",
        start.elapsed().as_millis(),
        total_count,
    );

    // 生成扁平化节点列表
    let flat_nodes = flatten_inspect_tree(&root_node);

    // 根据格式生成输出
    let text_output = if format == "txt" || format == "text" {
        Some(format_inspect_tree(&root_node, 0))
    } else {
        None
    };

    InspectResult {
        success: true,
        root_xpath: element_xpath.to_string(),
        nodes: Some(root_node),
        flat_nodes,
        text_output,
        total_children: total_count.saturating_sub(1), // 减去根节点自身
        error: None,
    }
}

fn build_inspect_node(
    element: &UIElement,
    walker: &UITreeWalker,
    depth: usize,
    max_depth: usize,
    max_nodes: usize,
    total_count: &mut usize,
    parent_xpath: &str,
) -> InspectNode {
    *total_count += 1;

    // 提取元素属性
    let control_type = element.get_control_type_raw()
        .map(control_type_name)
        .unwrap_or_default();
    let name = element.get_name().unwrap_or_default();
    let class_name = element.get_classname().unwrap_or_default();
    let automation_id = element.get_automation_id().unwrap_or_default();
    let framework_id = element.get_framework_id().unwrap_or_default();
    let help_text = element.get_help_text().unwrap_or_default();
    let item_type = element.get_item_type().unwrap_or_default();
    let item_status = element.get_item_status().unwrap_or_default();
    let is_offscreen = element.is_offscreen().unwrap_or(false);

    // 获取边界矩形
    let rect = match element.get_bounding_rectangle() {
        Ok(r) => Some(crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }),
        Err(_) => None,
    };

    // 尝试获取 ValuePattern 的文本内容
    let text_value = get_value_pattern_text(element);

    // 构建相对 XPath
    let relative_xpath = build_relative_xpath(
        &control_type,
        &name,
        &class_name,
        &automation_id,
        parent_xpath,
    );

    // 递归遍历子元素
    let mut children = Vec::new();
    if depth < max_depth && *total_count < max_nodes {
        // 首先收集所有直接子元素，以便计算同类型兄弟索引
        let child_elements: Vec<UIElement> = {
            let mut kids = Vec::new();
            let mut child = walker.get_first_child(element).ok();
            while let Some(c) = child {
                let last = c.clone();
                kids.push(c);
                child = walker.get_next_sibling(&last).ok();
            }
            kids
        };

        // 按控件类型统计出现次数，用于构建带索引的 XPath
        let mut type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for child_elem in &child_elements {
            let ct = child_elem.get_control_type_raw()
                .map(control_type_name)
                .unwrap_or_default();
            let idx = type_counts.entry(ct.clone()).or_insert(0);
            *idx += 1;
        }

        // 跟踪当前同类型已出现的次数
        let mut type_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for child_elem in &child_elements {
            if *total_count >= max_nodes {
                break;
            }
            let ct = child_elem.get_control_type_raw()
                .map(control_type_name)
                .unwrap_or_default();

            let seen = type_seen.entry(ct.clone()).or_insert(0);
            *seen += 1;
            let same_type_total = type_counts.get(&ct).copied().unwrap_or(1);

            // 为子节点构建带索引的 XPath 前缀
            let child_xpath = if same_type_total > 1 {
                format!("{}/{}[{}]", relative_xpath, ct, seen)
            } else {
                format!("{}/{}", relative_xpath, ct)
            };

            let mut child_node = build_inspect_node_inner(
                child_elem,
                walker,
                depth + 1,
                max_depth,
                max_nodes,
                total_count,
                &child_xpath,
                &name,
                &class_name,
                &automation_id,
            );
            child_node.relative_xpath = child_xpath;
            children.push(child_node);
        }
    }

    InspectNode {
        depth,
        control_type,
        name,
        class_name,
        automation_id,
        framework_id,
        text_value,
        help_text,
        item_type,
        item_status,
        rect,
        is_offscreen,
        relative_xpath: relative_xpath.to_string(),
        children,
    }
}

fn build_inspect_node_inner(
    element: &UIElement,
    walker: &UITreeWalker,
    depth: usize,
    max_depth: usize,
    max_nodes: usize,
    total_count: &mut usize,
    current_xpath: &str,
    _parent_name: &str,
    _parent_class: &str,
    _parent_aid: &str,
) -> InspectNode {
    *total_count += 1;

    let control_type = element.get_control_type_raw()
        .map(control_type_name)
        .unwrap_or_default();
    let name = element.get_name().unwrap_or_default();
    let class_name = element.get_classname().unwrap_or_default();
    let automation_id = element.get_automation_id().unwrap_or_default();
    let framework_id = element.get_framework_id().unwrap_or_default();
    let help_text = element.get_help_text().unwrap_or_default();
    let item_type = element.get_item_type().unwrap_or_default();
    let item_status = element.get_item_status().unwrap_or_default();
    let is_offscreen = element.is_offscreen().unwrap_or(false);

    let rect = match element.get_bounding_rectangle() {
        Ok(r) => Some(crate::core::model::Rect {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_right() - r.get_left(),
            height: r.get_bottom() - r.get_top(),
        }),
        Err(_) => None,
    };

    let text_value = get_value_pattern_text(element);

    // 递归遍历子元素
    let mut children = Vec::new();
    if depth < max_depth && *total_count < max_nodes {
        let child_elements: Vec<UIElement> = {
            let mut kids = Vec::new();
            let mut child = walker.get_first_child(element).ok();
            while let Some(c) = child {
                let last = c.clone();
                kids.push(c);
                if kids.len() >= max_nodes {
                    break;
                }
                child = walker.get_next_sibling(&last).ok();
            }
            kids
        };

        let mut type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for child_elem in &child_elements {
            let ct = child_elem.get_control_type_raw()
                .map(control_type_name)
                .unwrap_or_default();
            let idx = type_counts.entry(ct.clone()).or_insert(0);
            *idx += 1;
        }

        let mut type_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for child_elem in &child_elements {
            if *total_count >= max_nodes {
                break;
            }
            let ct = child_elem.get_control_type_raw()
                .map(control_type_name)
                .unwrap_or_default();

            let seen = type_seen.entry(ct.clone()).or_insert(0);
            *seen += 1;
            let same_type_total = type_counts.get(&ct).copied().unwrap_or(1);

            let child_xpath = if same_type_total > 1 {
                format!("{}/{}[{}]", current_xpath, ct, seen)
            } else {
                format!("{}/{}", current_xpath, ct)
            };

            let mut child_node = build_inspect_node_inner(
                child_elem,
                walker,
                depth + 1,
                max_depth,
                max_nodes,
                total_count,
                &child_xpath,
                &name,
                &class_name,
                &automation_id,
            );
            child_node.relative_xpath = child_xpath;
            children.push(child_node);
        }
    }

    InspectNode {
        depth,
        control_type,
        name,
        class_name,
        automation_id,
        framework_id,
        text_value,
        help_text,
        item_type,
        item_status,
        rect,
        is_offscreen,
        relative_xpath: String::new(), // 将由调用者设置
        children,
    }
}

fn get_value_pattern_text(element: &UIElement) -> Option<String> {
    use uiautomation::patterns::UIValuePattern;

    let value_pattern = element.get_pattern::<UIValuePattern>().ok()?;
    let value = value_pattern.get_value().ok()?;
    if value.is_empty() { None } else { Some(value) }
}

fn format_inspect_tree(node: &InspectNode, indent: usize) -> String {
    let mut lines = Vec::new();
    format_inspect_node_recursive(node, indent, &mut lines);
    lines.join("\n")
}

fn format_inspect_node_recursive(node: &InspectNode, indent: usize, lines: &mut Vec<String>) {
    // 判断节点是否有可识别信息：仅 name、text_value、help_text，且必须是非空有效字符串
    let has_identifiable_info = !node.name.is_empty()
        || node.text_value.as_ref().map_or(false, |s| !s.is_empty())
        || !node.help_text.is_empty();

    // 仅当有可识别信息时才输出该节点（否则只递归处理子节点）
    if has_identifiable_info {
        let prefix = "  ".repeat(indent);

        let mut parts = vec![format!("{}{}", prefix, node.control_type)];
        if !node.name.is_empty() {
            parts.push(format!("name=\"{}\"", node.name));
        }
        if !node.class_name.is_empty() {
            parts.push(format!("class=\"{}\"", node.class_name));
        }
        if !node.automation_id.is_empty() {
            parts.push(format!("id=\"{}\"", node.automation_id));
        }
        if let Some(ref text) = node.text_value {
            parts.push(format!("text=\"{}\"", text));
        }
        if !node.help_text.is_empty() {
            parts.push(format!("help=\"{}\"", node.help_text));
        }
        if !node.item_type.is_empty() {
            parts.push(format!("itemType=\"{}\"", node.item_type));
        }
        if !node.item_status.is_empty() {
            parts.push(format!("itemStatus=\"{}\"", node.item_status));
        }
        if let Some(ref rect) = node.rect {
            parts.push(format!("rect=({},{},{},{})", rect.x, rect.y, rect.width, rect.height));
        }
        if node.is_offscreen {
            parts.push("[offscreen]".to_string());
        }

        lines.push(parts.join(" "));

        for child in &node.children {
            format_inspect_node_recursive(child, indent + 1, lines);
        }
    } else {
        // 无可识别信息的节点不显示，但其子节点继承当前缩进层级
        for child in &node.children {
            format_inspect_node_recursive(child, indent, lines);
        }
    }
}

fn flatten_inspect_tree(root: &InspectNode) -> Vec<InspectNode> {
    let mut result = Vec::new();
    flatten_inspect_node_recursive(root, &mut result);
    result
}

fn flatten_inspect_node_recursive(node: &InspectNode, result: &mut Vec<InspectNode>) {
    let has_identifiable_info = !node.name.is_empty()
        || node.text_value.as_ref().map_or(false, |s| !s.is_empty())
        || !node.help_text.is_empty();

    if has_identifiable_info {
        let mut flat = node.clone();
        flat.children = vec![];
        result.push(flat);
    }
    for child in &node.children {
        flatten_inspect_node_recursive(child, result);
    }
}
