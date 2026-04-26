// src/xpath.rs
use crate::model::{HierarchyNode, XPathResult};

/// Build the complete XPath result from the captured hierarchy.
/// The hierarchy now starts from the Window node.
/// Returns:
///   - window_selector: XPath-like string for the window (first node)
///   - element_xpath: XPath for elements inside the window (starts with /)
pub fn generate(nodes: &[HierarchyNode]) -> XPathResult {
    if nodes.is_empty() {
        panic!("Hierarchy must not be empty");
    }
    
    // First node is always the Window
    let window_selector = nodes[0].xpath_segment();
    
    // Generate element XPath from nodes after Window (index 1+)
    let element_nodes = &nodes[1..];
    let element_xpath = generate_element_xpath(element_nodes);
    
    XPathResult {
        window_selector,
        element_xpath,
    }
}

/// Generate element XPath directly from element hierarchy (nodes after Window).
/// Uses "/" for direct children, "//" when intermediate nodes are skipped.
pub fn generate_elements(nodes: &[HierarchyNode]) -> String {
    generate_element_xpath(nodes)
}

/// Generate simplified element XPath from element hierarchy.
/// Keeps only nodes with AutomationId or Name, plus the target.
pub fn generate_simplified_elements(nodes: &[HierarchyNode]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    
    let last_idx = nodes.len() - 1;
    
    // Create a filtered version with simplified inclusion
    let simplified_nodes: Vec<HierarchyNode> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut node = n.clone();
            node.included = i == last_idx
                || (n.included && (!n.automation_id.is_empty() || !n.name.is_empty()));
            node
        })
        .collect();
    
    generate_element_xpath(&simplified_nodes)
}

/// Generate element XPath from nodes after the Window node.
/// Uses "/" for direct children, "//" when intermediate nodes are skipped.
fn generate_element_xpath(nodes: &[HierarchyNode]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    
    let included: Vec<&HierarchyNode> = nodes.iter()
        .filter(|n| n.included)
        .collect();
    
    if included.is_empty() {
        return String::new();
    }
    
    included.iter().enumerate().map(|(i, &node)| {
        if i == 0 {
            // First element node: always use /
            format!("/{}", node.xpath_segment())
        } else {
            // Check if we skipped any intermediate nodes
            let prev_node = included[i - 1];
            
            // Find original indices in the nodes slice
            let curr_idx = nodes.iter()
                .position(|n| std::ptr::eq(n, node))
                .unwrap();
            let prev_idx = nodes.iter()
                .position(|n| std::ptr::eq(n, prev_node))
                .unwrap();
            
            // If indices are not consecutive, nodes were skipped → use //
            if curr_idx > prev_idx + 1 {
                format!("//{}", node.xpath_segment())
            } else {
                format!("/{}", node.xpath_segment())
            }
        }
    }).collect::<Vec<_>>().join("")
}

/// Validate that `xpath` is syntactically well-formed enough to attempt a search.
/// Returns `None` if valid, or an error string describing the problem.
/// Supports both old format (//ControlType) and new format (Window[...], /ControlType).
pub fn lint(xpath: &str) -> Option<String> {
    let s = xpath.trim();
    if s.is_empty() {
        return Some("XPath 不能为空".into());
    }
    
    // Check if it's the new format: "window_selector, element_xpath"
    if let Some(comma_pos) = s.find(", ") {
        let window_part = &s[..comma_pos];
        let element_part = s[comma_pos + 2..].trim();
        
        // Validate window selector (should start with Window)
        if !window_part.starts_with("Window") {
            return Some("窗口选择器必须以 Window 开头".into());
        }
        
        // Validate element XPath (should start with /)
        if !element_part.starts_with('/') {
            return Some("元素 XPath 必须以 / 开头".into());
        }
        
        // Check balanced brackets in both parts
        for (name, part) in [("窗口选择器", window_part), ("元素 XPath", element_part)] {
            if let Some(err) = check_brackets(part) {
                return Some(format!("{} {}", name, err));
            }
        }
        
        return None;
    }
    
    // Old format: must start with //
    if !s.starts_with("//") {
        return Some("XPath 必须以 // 开头（或使用新格式：窗口选择器, 元素XPath）".into());
    }
    
    check_brackets(s)
}

/// Helper function to check balanced brackets.
fn check_brackets(s: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut str_ch = ' ';
    for ch in s.chars() {
        if in_str {
            if ch == str_ch { in_str = false; }
        } else if ch == '\'' || ch == '"' {
            in_str = true;
            str_ch = ch;
        } else if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth < 0 {
                return Some("方括号不匹配（多余的 ]）".into());
            }
        }
    }
    if depth != 0 {
        return Some(format!("方括号不匹配（缺少 {} 个 ]）", depth));
    }
    if in_str {
        return Some("字符串引号未闭合".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ElementRect, HierarchyNode};

    fn node(ct: &str, aid: &str, name: &str) -> HierarchyNode {
        HierarchyNode::new(ct, aid, "", name, 0, ElementRect::default(), 0)
    }

    #[test]
    fn basic_generation() {
        // Hierarchy now starts from Window
        let nodes = vec![
            node("Window", "main", ""),
            node("Button", "btnOk", "OK")
        ];
        let result = generate(&nodes);
        
        // Window selector should not start with //
        assert!(!result.window_selector.starts_with("//"));
        assert!(result.window_selector.starts_with("Window"));
        assert!(result.window_selector.contains("AutomationId='main'"));
        
        // Element XPath should start with /
        assert!(result.element_xpath.starts_with("/Button"));
        assert!(result.element_xpath.contains("AutomationId='btnOk'"));
    }

    #[test]
    fn simplify_drops_unnamed() {
        // Create full hierarchy (Window + elements)
        let full_nodes = vec![
            node("Window",  "main",  ""),
            node("Pane",    "",      ""),     // no id or name → dropped
            node("Button",  "btn1",  "Save"),
        ];
        
        // Generate window selector from full hierarchy
        let window_selector = full_nodes[0].xpath_segment();
        
        // Generate simplified element XPath from element nodes (after Window)
        let element_nodes = &full_nodes[1..];
        let element_xpath = generate_simplified_elements(element_nodes);
        
        // Window selector should be present
        assert!(window_selector.contains("Window"));
        
        // Element XPath should drop unnamed Pane
        assert!(!element_xpath.contains("Pane"), "unnamed Pane should be dropped");
        assert!(element_xpath.contains("/Button"));
    }

    #[test]
    fn lint_catches_imbalance() {
        // Old format
        assert!(lint("//Button[@AutomationId='x'").is_some());
        assert!(lint("//Button[@AutomationId='x']").is_none());
        assert!(lint("").is_some());
        assert!(lint("Button[@id='x']").is_some());
        
        // New format
        assert!(lint("Window[@Name='test'], /Button[@id='x']").is_none());
        assert!(lint("Button[@Name='test'], /Button[@id='x']").is_some()); // Must start with Window
        assert!(lint("Window[@Name='test'], Button[@id='x']").is_some()); // Element must start with /
    }

    // ─── PDCA: WeChat real-world XPath validation ────────────────────────────────

    /// Test that generated XPath follows the new format: window_selector + element_xpath
    #[test]
    fn wechat_xpath_format() {
        // Simulate the captured WeChat hierarchy (starts from Window, 8 nodes)
        let nodes = vec![
            HierarchyNode::new("Window", "", "mmui::MainWindow", "微信", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "QWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Custom", "", "QStackedWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "MainView", "mmui::MainView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("ToolBar", "MainView.main_tabbar", "mmui::MainTabBar", "导航", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "mmui::XTabBarItem", "搜一搜", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "QWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "mmui::XImage", "", 1, ElementRect::default(), 0),
        ];

        let result = generate(&nodes);
        
        // Verify window selector
        assert!(result.window_selector.starts_with("Window"), "Window selector must start with Window");
        assert!(result.window_selector.contains("ClassName='mmui::MainWindow'"));
        
        // Verify element XPath starts with /
        assert!(result.element_xpath.starts_with("/Group"), "Element XPath must start with /");
        assert!(result.element_xpath.contains("/Custom"), "Should have /Custom");
        assert!(result.element_xpath.contains("/Group["), "Should have /Group");
        assert!(result.element_xpath.contains("/ToolBar["), "Should have /ToolBar");
        assert!(result.element_xpath.contains("/Button["), "Should have /Button");
        
        // Verify no // in element XPath (all nodes included)
        assert!(!result.element_xpath.contains("//"), "Should not have // when all nodes included");
        
        println!("Window selector:\n{}", result.window_selector);
        println!("Element XPath:\n{}", result.element_xpath);
    }

    /// Test XPath with skipped nodes (should use //)
    #[test]
    fn skipped_nodes_use_double_slash() {
        let nodes = vec![
            node("Window", "main", ""),
            HierarchyNode::new("Group", "", "Class1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "Class2", "", 0, ElementRect::default(), 0),  // Will be excluded
            HierarchyNode::new("Button", "btn1", "", "", 0, ElementRect::default(), 0),
        ];
        
        // Exclude the Pane node (index 2)
        let mut nodes = nodes;
        nodes[2].included = false;
        
        let result = generate(&nodes);
        
        // Should have // because Pane was skipped
        assert!(result.element_xpath.contains("//Button"), "Should use // when intermediate node skipped");
        assert!(result.element_xpath.contains("/Group"), "Should start with /Group");
        
        println!("XPath with skipped node:\n{}", result.element_xpath);
    }

    /// Test that Index predicates use position() function
    #[test]
    fn position_function_generation() {
        let nodes = vec![
            node("Window", "main", ""),
            HierarchyNode::new("Button", "", "", "", 1, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 2, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 3, ElementRect::default(), 0),
        ];

        let result = generate(&nodes);
        
        // Check that first() is used for position 1
        assert!(result.element_xpath.contains("first()"), "Should use first() for position 1");
        
        // Check that position()=N is used for middle elements
        assert!(result.element_xpath.contains("position()=2"), "Should use position()=2 for middle element");
        
        println!("XPath with position functions:\n{}", result.element_xpath);
    }

    /// Test disabling position() function (fallback to @Index)
    #[test]
    fn index_fallback_without_position_function() {
        let mut node = HierarchyNode::new("Button", "", "", "", 2, ElementRect::default(), 0);
        node.position_mode = "index".to_string();
        
        let segment = node.xpath_segment();
        
        // Should use @Index when position mode is "index"
        assert!(segment.contains("@Index='2'"), "Should fallback to @Index when mode is index");
        assert!(!segment.contains("position()"), "Should not use position() when mode is index");
        
        println!("XPath segment with @Index:\n{}", segment);
    }

    /// Test first() function
    #[test]
    fn first_function_generation() {
        let mut node = HierarchyNode::new("Button", "", "", "", 1, ElementRect::default(), 0);
        node.sibling_count = 3;
        
        let segment = node.xpath_segment();
        
        // Default mode should use first() for position 1
        assert!(segment.contains("first()"), "Should use first() for position 1");
        assert!(!segment.contains("position()"), "Should not use position()=1");
        
        println!("XPath segment with first():\n{}", segment);
    }

    /// Test last() function
    #[test]
    fn last_function_generation() {
        let mut node = HierarchyNode::new("Button", "", "", "", 3, ElementRect::default(), 0);
        node.sibling_count = 3;  // Last of 3 siblings
        
        let segment = node.xpath_segment();
        
        // Default mode should use last() when position equals sibling_count
        assert!(segment.contains("last()"), "Should use last() for last sibling");
        assert!(!segment.contains("position()"), "Should not use position() for last");
        
        println!("XPath segment with last():\n{}", segment);
    }

    /// Test manual first() and last() mode override
    #[test]
    fn position_mode_override() {
        // Test manual first() override
        let mut node1 = HierarchyNode::new("Button", "", "", "", 2, ElementRect::default(), 0);
        node1.position_mode = "first".to_string();
        let segment1 = node1.xpath_segment();
        assert!(segment1.contains("first()"), "Manual first() mode should always use first()");
        
        // Test manual last() override
        let mut node2 = HierarchyNode::new("Button", "", "", "", 1, ElementRect::default(), 0);
        node2.position_mode = "last".to_string();
        let segment2 = node2.xpath_segment();
        assert!(segment2.contains("last()"), "Manual last() mode should always use last()");
        
        println!("Manual first(): {}", segment1);
        println!("Manual last(): {}", segment2);
    }

    /// Test new operators: NotContains, NotStartsWith, NotEndsWith
    #[test]
    fn new_string_operators() {
        use crate::model::Operator;
        
        // NotContains
        let pred = Operator::NotContains.to_predicate("Name", "test");
        assert!(pred.contains("not(contains("), "NotContains should use not(contains())");
        
        // NotStartsWith
        let pred = Operator::NotStartsWith.to_predicate("Name", "test");
        assert!(pred.contains("not(starts-with("), "NotStartsWith should use not(starts-with())");
        
        // NotEndsWith
        let pred = Operator::NotEndsWith.to_predicate("Name", "test");
        assert!(pred.contains("not(substring("), "NotEndsWith should use not(substring())");
        
        println!("NotContains: {}", Operator::NotContains.to_predicate("Name", "test"));
        println!("NotStartsWith: {}", Operator::NotStartsWith.to_predicate("Name", "test"));
        println!("NotEndsWith: {}", Operator::NotEndsWith.to_predicate("Name", "test"));
    }

    /// Test numeric comparison operators
    #[test]
    fn numeric_comparison_operators() {
        use crate::model::Operator;
        
        // GreaterThan
        assert_eq!(Operator::GreaterThan.to_predicate("Index", "5"), "@Index > 5");
        
        // GreaterThanOrEq
        assert_eq!(Operator::GreaterThanOrEq.to_predicate("Index", "5"), "@Index >= 5");
        
        // LessThan
        assert_eq!(Operator::LessThan.to_predicate("Index", "5"), "@Index < 5");
        
        // LessThanOrEq
        assert_eq!(Operator::LessThanOrEq.to_predicate("Index", "5"), "@Index <= 5");
        
        println!("GreaterThan: {}", Operator::GreaterThan.to_predicate("Index", "5"));
        println!("GreaterThanOrEq: {}", Operator::GreaterThanOrEq.to_predicate("Index", "5"));
        println!("LessThan: {}", Operator::LessThan.to_predicate("Index", "5"));
        println!("LessThanOrEq: {}", Operator::LessThanOrEq.to_predicate("Index", "5"));
    }

    /// Performance test: verify that using / instead of // reduces complexity
    #[test]
    fn xpath_optimization_impact() {
        // Simulate a deep hierarchy with Window at root
        let mut nodes: Vec<HierarchyNode> = vec![
            node("Window", "main", "")
        ];
        
        for i in 0..9 {
            nodes.push(
                HierarchyNode::new("Pane", "", &format!("Class{}", i), "", i, ElementRect::default(), 0)
            );
        }

        let result = generate(&nodes);
        
        // Count / vs // to verify optimization
        let single_slash_count = result.element_xpath.matches('/').count();
        let double_slash_count = result.element_xpath.matches("//").count();
        
        // Should have no double slashes (all nodes included)
        assert_eq!(double_slash_count, 0, "Should have no // when all nodes included");
        
        // Should have 9 single slashes for 9 segments
        assert_eq!(single_slash_count, 9, "Should have 9 single slashes for 9 segments");
        
        println!("Optimized XPath ({} segments):\n{}", nodes.len() - 1, result.element_xpath);
        println!("Complexity: {} single / = much faster than all //", single_slash_count);
    }
}
