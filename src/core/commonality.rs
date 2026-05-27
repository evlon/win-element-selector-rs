// src/core/commonality.rs
//
// 多元素共同特征提取：从多个相似元素样本中提取共同祖先链，生成搜索 XPath
//
// 算法：逐层对比，每层只保留所有样本都相同的属性，不同的属性去掉。
// 分叉后相同的层级也要继续往下走，不是找到第一个分叉就停。

use std::collections::HashSet;

use crate::core::model::{CommonAncestorPath, HierarchyNode, SimilarElementSample, WindowInfo};

/// 共同链中的节点：每个层级提取出所有样本共有的属性
#[derive(Debug, Clone)]
struct CommonLevel {
    control_type: String,
    common_predicates: Vec<(String, String)>,
    depth_from_window: usize,
}

/// 从多个样本中提取共同祖先路径
pub fn extract_common_path(
    samples: &[SimilarElementSample],
    window_info: Option<&WindowInfo>,
    ignore_numeric_aid: bool,
) -> Option<CommonAncestorPath> {
    if samples.len() < 2 {
        return None;
    }

    // 1. 检查所有样本的目标元素类型是否一致
    let target_type = &samples[0].hierarchy_node.control_type;
    if !samples.iter().all(|s| s.hierarchy_node.control_type == *target_type) {
        log::warn!("[commonality] 目标元素类型不一致，target_type={}", target_type);
        return None;
    }

    // 2. 收集所有目标元素的名字，用于过滤祖先节点
    let target_names: HashSet<&str> = samples.iter()
        .map(|s| s.hierarchy_node.name.as_str())
        .filter(|n| !n.is_empty())
        .collect();

    // 3. 找到窗口节点在 chain 中的索引
    let inside_start = find_window_end_index(&samples[0].ancestor_chain, window_info);

    // DEBUG: 打印每个样本的 ancestor_chain
    for (si, s) in samples.iter().enumerate() {
        log::info!("[commonality] 样本 {} (chain_len={}, inside_start={}):",
            si, s.ancestor_chain.len(), inside_start);
        for (ni, node) in s.ancestor_chain.iter().enumerate() {
            log::info!("[commonality]   [{}] {} aid='{}' name='{}' class='{}' framework='{}'",
                ni, node.control_type, node.automation_id, node.name,
                node.class_name, node.framework_id);
        }
    }

    // 4. 逐层对比，提取每一层的共同属性（不包含目标节点本身）
    let min_chain_len = samples.iter().map(|s| s.ancestor_chain.len()).min().unwrap_or(0);
    let mut common_levels: Vec<CommonLevel> = Vec::new();

    for i in inside_start..min_chain_len {
        let ref_node = &samples[0].ancestor_chain[i];

        // 遇到目标类型就停止（ancestor_chain 不应包含目标节点）
        if ref_node.control_type == *target_type {
            log::info!("[commonality] 索引 {} 遇到目标类型 {}，停止", i, target_type);
            break;
        }

        // 跳过名字匹配任何目标名字的祖先节点
        if !ref_node.name.is_empty() && target_names.contains(ref_node.name.as_str()) {
            log::info!("[commonality] 跳过索引 {} 的祖先节点 {} (name='{}') — 匹配目标名字",
                i, ref_node.control_type, ref_node.name);
            continue;
        }

        // 检查所有样本在该位置的 control_type
        let all_same_type = samples.iter().all(|s| {
            s.ancestor_chain.get(i).map_or(false, |n| n.control_type == ref_node.control_type)
        });

        if !all_same_type {
            log::info!("[commonality] 索引 {} 处 control_type 不同，停止", i);
            break;
        }

        // control_type 相同，动态提取所有样本在该位置的共同属性
        let common_predicates = compute_common_properties(&samples, i, ignore_numeric_aid);

        common_levels.push(CommonLevel {
            control_type: ref_node.control_type.clone(),
            common_predicates,
            depth_from_window: ref_node.depth_from_window,
        });
    }

    log::info!("[commonality] 共同层级数: {}", common_levels.len());

    // 5. 生成 XPath
    let search_xpath = build_search_xpath_from_levels(&common_levels, target_type);

    Some(CommonAncestorPath {
        common_ancestors: common_levels.iter().map(|l| samples[0].ancestor_chain
            .iter()
            .find(|n| n.control_type == l.control_type)
            .cloned()
            .unwrap_or_else(|| samples[0].ancestor_chain[inside_start].clone())
        ).collect(),
        target_control_type: target_type.clone(),
        search_xpath,
    })
}

/// 动态提取所有样本在索引 i 处的共同属性。
/// 遍历 HierarchyNode 的所有可用作 XPath 谓词的属性，只保留所有样本都相同的。
fn compute_common_properties(samples: &[SimilarElementSample], i: usize, ignore_numeric_aid: bool) -> Vec<(String, String)> {
    let ref_node = &samples[0].ancestor_chain[i];
    let mut predicates = Vec::new();

    // 定义可比较的属性列表及提取函数
    // 按优先级排列：AutomationId 最稳定，其次是 ClassName 等
    // 注意：LocalizedControlType 是系统语言相关的（中文返回"按钮"，英文返回"Button"），
    // 会导致 XPath 不可移植，因此不加入。ControlType 已通过 XPath 标签名表达，也不重复。
    let props: Vec<(&str, &dyn Fn(&HierarchyNode) -> String)> = vec![
        ("AutomationId", &|n: &HierarchyNode| n.automation_id.clone()),
        ("ClassName", &|n: &HierarchyNode| n.class_name.clone()),
        ("FrameworkId", &|n: &HierarchyNode| if !n.framework_id.is_empty() { n.framework_id.clone() } else { String::new() }),
        ("Name", &|n: &HierarchyNode| n.name.clone()),
        ("HelpText", &|n: &HierarchyNode| if !n.help_text.is_empty() { n.help_text.clone() } else { String::new() }),
        ("ItemType", &|n: &HierarchyNode| if !n.item_type.is_empty() { n.item_type.clone() } else { String::new() }),
        ("ItemStatus", &|n: &HierarchyNode| if !n.item_status.is_empty() { n.item_status.clone() } else { String::new() }),
        ("AccessKey", &|n: &HierarchyNode| if !n.access_key.is_empty() { n.access_key.clone() } else { String::new() }),
        ("AcceleratorKey", &|n: &HierarchyNode| if !n.accelerator_key.is_empty() { n.accelerator_key.clone() } else { String::new() }),
    ];

    for (prop_name, extractor) in &props {
        let ref_val = extractor(ref_node);
        if ref_val.is_empty() {
            continue;
        }

        // AutomationId：纯数字且配置忽略则跳过
        if *prop_name == "AutomationId" && ignore_numeric_aid && is_numeric_id(&ref_val) {
            continue;
        }

        // Name：如果 ref_node 的 name 匹配目标名字，跳过（已在上面 skip 了）
        // 但这里是共同属性提取，不需要额外过滤

        // 检查所有样本在该位置是否有相同的值
        let all_same = samples.iter().skip(1).all(|s| {
            s.ancestor_chain.get(i).map_or(false, |n| extractor(n) == ref_val)
        });

        if all_same {
            predicates.push((prop_name.to_string(), ref_val));
        }
    }

    predicates
}

/// 找到窗口节点在 ancestor_chain 中的结束索引（即窗口节点的下一个位置）
/// 返回从该索引开始，后面的都是窗口内的节点
fn find_window_end_index(chain: &[HierarchyNode], window_info: Option<&WindowInfo>) -> usize {
    if let Some(win) = window_info {
        if !win.title.is_empty() {
            // 在 chain 中找到 name == window_info.title 的节点
            for (i, node) in chain.iter().enumerate() {
                if node.name == win.title {
                    log::info!("[commonality] 窗口节点在索引 {} (name='{}')，从索引 {} 开始比较",
                        i, win.title, i + 1);
                    return i + 1;
                }
            }
        }
    }
    // 降级：跳过索引 0（桌面根节点），从索引 1 开始
    if chain.len() > 1 {
        return 1;
    }
    0
}

/// 转义 XPath 字符串中的单引号
fn escape_xpath_value(s: &str) -> String {
    s.replace('\'', "\\'")
}

/// 判断是否为纯数字 ID（大概率是随机生成的）
fn is_numeric_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// 从共同层级列表构建 XPath
/// 策略：
/// - 相邻层级 depth 差值 = 1 → 直接父子，用 `/`
/// - depth 差值 > 1 → 中间有其他层，用 `//`
/// - 最后一个锚点到目标用 `//`
fn build_search_xpath_from_levels(levels: &[CommonLevel], target_type: &str) -> String {
    if levels.is_empty() {
        return format!("//{}", target_type);
    }

    let segments: Vec<String> = levels.iter()
        .map(|l| level_to_xpath_segment(l))
        .collect();

    // 构建连接符：根据 depth_from_window 差值决定 / 或 //
    let mut parts: Vec<String> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        if i == 0 {
            parts.push(seg.clone());
        } else {
            let prev_level = &levels[i - 1];
            let curr_level = &levels[i];
            // depth 差值 = 1 表示直接父子，否则是跨层
            let depth_diff = curr_level.depth_from_window as isize - prev_level.depth_from_window as isize;
            let connector = if depth_diff == 1 { "/" } else { "//" };
            parts.push(format!("{}{}", connector, seg));
        }
    }

    // 最后一个层级到目标用 //（descendant）
    format!("//{}//{}", parts.join(""), target_type)
}

/// 将共同层级转为 XPath 片段
fn level_to_xpath_segment(level: &CommonLevel) -> String {
    let ct = escape_xpath_value(&level.control_type);

    if level.common_predicates.is_empty() {
        return ct;
    }

    // 构建谓词：优先使用 AutomationId（最稳定），其次是其他属性
    let predicates: Vec<String> = level.common_predicates.iter()
        .map(|(name, value)| {
            let v = escape_xpath_value(value);
            format!("@{}='{}'", name, v)
        })
        .collect();

    format!("{}[{}]", ct, predicates.join(" and "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ElementRect;

    fn make_node(control_type: &str, automation_id: &str, name: &str) -> HierarchyNode {
        HierarchyNode {
            control_type: control_type.to_string(),
            automation_id: automation_id.to_string(),
            class_name: String::new(),
            name: name.to_string(),
            index: 0,
            framework_id: String::new(),
            acc_role: String::new(),
            help_text: String::new(),
            localized_control_type: String::new(),
            is_enabled: true,
            is_offscreen: false,
            is_password: false,
            accelerator_key: String::new(),
            access_key: String::new(),
            item_type: String::new(),
            item_status: String::new(),
            rect: ElementRect { x: 0, y: 0, width: 100, height: 50 },
            process_id: 0,
            filters: vec![],
            included: true,
            is_target: false,
            position_mode: String::new(),
            sibling_count: 1,
            depth_from_window: 0,
        }
    }

    fn make_node_full(control_type: &str, automation_id: &str, class_name: &str, name: &str, framework_id: &str, localized_control_type: &str) -> HierarchyNode {
        HierarchyNode {
            control_type: control_type.to_string(),
            automation_id: automation_id.to_string(),
            class_name: class_name.to_string(),
            name: name.to_string(),
            index: 0,
            framework_id: framework_id.to_string(),
            acc_role: String::new(),
            help_text: String::new(),
            localized_control_type: localized_control_type.to_string(),
            is_enabled: true,
            is_offscreen: false,
            is_password: false,
            accelerator_key: String::new(),
            access_key: String::new(),
            item_type: String::new(),
            item_status: String::new(),
            rect: ElementRect { x: 0, y: 0, width: 100, height: 50 },
            process_id: 0,
            filters: vec![],
            included: true,
            is_target: false,
            position_mode: String::new(),
            sibling_count: 1,
            depth_from_window: 0,
        }
    }

    fn make_sample_with_name(target_type: &str, target_name: &str, chain: Vec<HierarchyNode>) -> SimilarElementSample {
        SimilarElementSample {
            hierarchy_node: make_node(target_type, "", target_name),
            ancestor_chain: chain,
            children_structure: vec![],
        }
    }

    fn make_sample(target_type: &str, chain: Vec<HierarchyNode>) -> SimilarElementSample {
        make_sample_with_name(target_type, "", chain)
    }

    fn win_info(title: &str) -> WindowInfo {
        WindowInfo {
            title: title.to_string(),
            class_name: "TestWindow".to_string(),
            process_id: 12345,
            process_name: "test".to_string(),
        }
    }

    #[test]
    fn test_window_node_stripped() {
        let chain = vec![
            make_node("Pane", "", "桌面 1"),
            make_node("Pane", "", "微信"),
            make_node("Document", "68701072", "搜一搜"),
        ];
        let s1 = make_sample_with_name("Button", "搜一搜", chain.clone());
        let s2 = make_sample_with_name("Button", "聊天", chain);

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), true).unwrap();
        assert!(!result.search_xpath.contains("Pane"), "不应包含窗口节点: {}", result.search_xpath);
    }

    #[test]
    fn test_different_name_per_level_keeps_common_type() {
        let chain = vec![
            make_node("Window", "", "微信"),
            make_node("Group", "MainView", ""),
            make_node("ToolBar", "MainView.main_tabbar", ""),
        ];
        let s1 = make_sample_with_name("Button", "搜一搜", chain.clone());
        let s2 = make_sample_with_name("Button", "聊天", chain);

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), true).unwrap();
        assert_eq!(result.target_control_type, "Button");
        assert_eq!(result.common_ancestors.len(), 2);
        assert!(result.search_xpath.contains("Group[@AutomationId='MainView']"));
        assert!(result.search_xpath.contains("ToolBar[@AutomationId='MainView.main_tabbar']"));
        assert!(result.search_xpath.ends_with("Button"));
    }

    #[test]
    fn test_different_control_types_returns_none() {
        let chain = vec![
            make_node("Window", "", "微信"),
            make_node("Group", "MainView", ""),
        ];
        let s1 = make_sample("Button", chain.clone());
        let s2 = make_sample("Text", chain);

        assert!(extract_common_path(&[s1, s2], Some(&win_info("微信")), true).is_none());
    }

    #[test]
    fn test_ignore_numeric_automation_id() {
        let chain1 = vec![
            make_node("Window", "", "微信"),
            make_node("Group", "12345", ""),
            make_node("ToolBar", "tabbar", ""),
        ];
        let s1 = make_sample("Button", chain1);

        let chain2 = vec![
            make_node("Window", "", "微信"),
            make_node("Group", "67890", ""),
            make_node("ToolBar", "tabbar", ""),
        ];
        let s2 = make_sample("Button", chain2);

        let result = extract_common_path(&[s1.clone(), s2.clone()], Some(&win_info("微信")), true).unwrap();
        assert_eq!(result.common_ancestors.len(), 2);
        assert_eq!(result.common_ancestors[0].control_type, "Group");
        assert!(!result.search_xpath.contains("12345"));
        assert!(!result.search_xpath.contains("67890"));

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), false).unwrap();
        assert_eq!(result.common_ancestors.len(), 2);
        assert!(!result.search_xpath.contains("12345"));
    }

    #[test]
    fn test_no_window_info_fallback() {
        let chain = vec![
            make_node("Window", "", "微信"),
            make_node("Group", "MainView", ""),
            make_node("ToolBar", "MainView.main_tabbar", ""),
        ];
        let s1 = make_sample_with_name("Button", "搜一搜", chain.clone());
        let s2 = make_sample_with_name("Button", "聊天", chain);

        let result = extract_common_path(&[s1, s2], None, true).unwrap();
        assert_eq!(result.common_ancestors.len(), 2);
    }

    #[test]
    fn test_filters_ancestor_matching_target_name() {
        let chain = vec![
            make_node("Window", "", "微信"),
            make_node("Pane", "", "搜一搜"),
        ];
        let s1 = make_sample_with_name("Button", "搜一搜", chain.clone());
        let s2 = make_sample_with_name("Button", "聊天", chain);

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), true).unwrap();
        assert!(result.common_ancestors.is_empty());
        assert_eq!(result.search_xpath, "//Button");
    }

    #[test]
    fn test_wechat_real_scenario() {
        let chain = vec![
            make_node("Pane", "", "桌面 1"),
            make_node("Pane", "", "微信"),
            make_node("Document", "68701072", ""),
        ];
        let s1 = make_sample_with_name("Button", "聊天", chain.clone());
        let s2 = make_sample_with_name("Button", "搜一搜", chain);

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), true).unwrap();
        assert!(!result.search_xpath.contains("Pane"), "不应包含窗口节点: {}", result.search_xpath);
    }

    #[test]
    fn test_dynamic_common_properties() {
        // 模拟真实微信场景：Group 和 Custom 没有 AutomationId/Name，但有共同的 ClassName/FrameworkId
        let chain1 = vec![
            make_node("Window", "", "微信"),
            make_node_full("Group", "", "QWidget", "", "Qt", "组"),
            make_node_full("Custom", "", "QStackedWidget", "", "Qt", "自定义"),
            make_node_full("Group", "MainView", "mmui::MainView", "", "Qt", "组"),
            make_node_full("ToolBar", "MainView.main_tabbar", "mmui::MainTabBar", "导航", "Qt", "工具栏"),
        ];
        let s1 = make_sample_with_name("Button", "收藏", chain1.clone());

        let chain2 = vec![
            make_node("Window", "", "微信"),
            make_node_full("Group", "", "QWidget", "", "Qt", "组"),
            make_node_full("Custom", "", "QStackedWidget", "", "Qt", "自定义"),
            make_node_full("Group", "MainView", "mmui::MainView", "", "Qt", "组"),
            make_node_full("ToolBar", "MainView.main_tabbar", "mmui::MainTabBar", "导航", "Qt", "工具栏"),
        ];
        let s2 = make_sample_with_name("Button", "朋友圈", chain2);

        let result = extract_common_path(&[s1, s2], Some(&win_info("微信")), true).unwrap();
        // Group 和 Custom 应该包含 ClassName/FrameworkId/LocalizedControlType 谓词
        assert!(result.search_xpath.contains("@ClassName='QWidget'"), "应包含 QWidget: {}", result.search_xpath);
        assert!(result.search_xpath.contains("@ClassName='QStackedWidget'"), "应包含 QStackedWidget: {}", result.search_xpath);
        assert!(result.search_xpath.contains("@FrameworkId='Qt'"), "应包含 FrameworkId: {}", result.search_xpath);
    }
}
