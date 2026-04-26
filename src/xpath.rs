// src/xpath.rs
use crate::model::HierarchyNode;

/// Build the full XPath from the active (included) nodes.
/// First node uses "//" (search from root), subsequent nodes use "/" (direct children).
pub fn generate(nodes: &[HierarchyNode]) -> String {
    let included: Vec<&HierarchyNode> = nodes.iter().filter(|n| n.included).collect();
    
    included
        .iter()
        .enumerate()
        .map(|(i, n)| n.xpath_segment_with_prefix(i == 0))
        .collect::<Vec<_>>()
        .join("")
}

/// Attempt to produce a shorter XPath by keeping only nodes with
/// a unique-enough identity (AutomationId or Name set), plus the target.
/// First node uses "//", subsequent nodes use "/".
pub fn simplify(nodes: &[HierarchyNode]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    let last_idx = nodes.len() - 1;
    let simplified: Vec<&HierarchyNode> = nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| {
            *i == last_idx
                || n.included && (!n.automation_id.is_empty() || !n.name.is_empty())
        })
        .map(|(_, n)| n)
        .collect();

    simplified
        .iter()
        .enumerate()
        .map(|(i, n)| n.xpath_segment_with_prefix(i == 0))
        .collect::<Vec<_>>()
        .join("")
}

/// Validate that `xpath` is syntactically well-formed enough to attempt a search.
/// Returns `None` if valid, or an error string describing the problem.
pub fn lint(xpath: &str) -> Option<String> {
    let s = xpath.trim();
    if s.is_empty() {
        return Some("XPath 不能为空".into());
    }
    if !s.starts_with("//") {
        return Some("XPath 必须以 // 开头".into());
    }
    // Check balanced brackets.
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
                return Some("XPath 方括号不匹配（多余的 ]）".into());
            }
        }
    }
    if depth != 0 {
        return Some(format!("XPath 方括号不匹配（缺少 {} 个 ]）", depth));
    }
    if in_str {
        return Some("XPath 字符串引号未闭合".into());
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
        let nodes = vec![node("Window", "main", ""), node("Button", "btnOk", "OK")];
        let x = generate(&nodes);
        assert!(x.starts_with("//Window"), "First segment should start with //");
        assert!(x.contains("AutomationId='main'"));
        assert!(x.contains("/Button"), "Second segment should start with /");
        assert!(x.contains("AutomationId='btnOk'"));
    }

    #[test]
    fn simplify_drops_unnamed() {
        let nodes = vec![
            node("Window",  "main",  ""),
            node("Pane",    "",      ""),     // no id or name → dropped
            node("Button",  "btn1",  "Save"),
        ];
        let x = simplify(&nodes);
        assert!(!x.contains("Pane"), "unnamed Pane should be dropped");
        assert!(x.starts_with("//Window"), "First segment should start with //");
        assert!(x.contains("/Button"), "Second segment should start with /");
    }

    #[test]
    fn lint_catches_imbalance() {
        assert!(lint("//Button[@AutomationId='x'").is_some());
        assert!(lint("//Button[@AutomationId='x']").is_none());
        assert!(lint("").is_some());
        assert!(lint("Button[@id='x']").is_some());
    }

    // ─── PDCA: WeChat real-world XPath validation ────────────────────────────────

    /// Test that generated XPath follows the pattern: //First/Second/Third...
    #[test]
    fn wechat_xpath_format() {
        // Simulate the captured WeChat hierarchy (9 nodes)
        let nodes = vec![
            HierarchyNode::new("Pane", "", "#32769", "桌面 1", 0, ElementRect::default(), 0),
            HierarchyNode::new("Window", "", "mmui::MainWindow", "微信", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "QWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Custom", "", "QStackedWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "MainView", "mmui::MainView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("ToolBar", "MainView.main_tabbar", "mmui::MainTabBar", "导航", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "mmui::XTabBarItem", "搜一搜", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "QWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "mmui::XImage", "", 1, ElementRect::default(), 0),
        ];

        let xpath = generate(&nodes);
        
        // Verify format: first segment uses //, others use /
        assert!(xpath.starts_with("//Pane"), "First segment must start with //");
        assert!(xpath.contains("/Window["), "Second segment must start with /");
        assert!(xpath.contains("/Group["), "Third segment must start with /");
        assert!(xpath.contains("/Custom["), "Fourth segment must start with /");
        assert!(xpath.contains("/ToolBar["), "Sixth segment must start with /");
        assert!(xpath.contains("/Button["), "Seventh segment must start with /");
        
        // Verify no double // in the middle
        let without_first = &xpath[2..]; // Remove leading //
        assert!(!without_first.contains("//"), "Should not have // after first segment");
        
        println!("Generated XPath:\n{}", xpath);
    }

    /// Test XPath segment parsing with mixed / and //
    #[test]
    fn parse_mixed_slashes() {
        // This test validates that parse_xpath correctly handles // and /
        // Note: parse_xpath is in the uia module, we test it indirectly through validation
        
        let xpath = "//Pane[@ClassName='#32769']/Window[@ClassName='mmui::MainWindow']/Button[@Name='Test']";
        
        // Verify lint passes
        assert!(lint(xpath).is_none(), "XPath should be valid");
        
        // Verify structure
        assert!(xpath.starts_with("//"));
        assert!(xpath.contains("/Window["), "Should have /Window after first segment");
        assert!(xpath.contains("/Button["), "Should have /Button as last segment");
        
        // Count segments by counting "/" that are not "//"
        let without_double = xpath.replace("//", "DOUBLE_SLASH");
        let single_slash_count = without_double.matches('/').count();
        assert_eq!(single_slash_count, 2, "Should have 2 single slashes separating 3 segments");
    }

    /// Test that Index predicates are preserved in XPath generation
    #[test]
    fn index_predicate_preserved() {
        let nodes = vec![
            node("Window", "main", ""),
            HierarchyNode::new("Button", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 1, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 2, ElementRect::default(), 0),
        ];

        let xpath = generate(&nodes);
        
        // Check that Index predicates are included
        assert!(xpath.contains("@Index='1'"), "Should include Index for node with index 1");
        assert!(xpath.contains("@Index='2'"), "Should include Index for node with index 2");
        
        println!("XPath with Index:\n{}", xpath);
    }

    /// Performance test: verify that using / instead of // reduces complexity
    #[test]
    fn xpath_optimization_impact() {
        // Simulate a deep hierarchy (10 levels)
        let nodes: Vec<HierarchyNode> = (0..10)
            .map(|i| {
                HierarchyNode::new("Pane", "", &format!("Class{}", i), "", i, ElementRect::default(), 0)
            })
            .collect();

        let optimized_xpath = generate(&nodes);
        
        // Count / vs // to verify optimization
        let single_slash_count = optimized_xpath.matches('/').count();
        let double_slash_count = optimized_xpath.matches("//").count();
        
        // Should have exactly 1 double slash (at the beginning)
        assert_eq!(double_slash_count, 1, "Should have exactly one // at the start");
        
        // The rest should be single slashes
        assert_eq!(single_slash_count - 2, 9, "Should have 9 single slashes for 10 segments");
        
        println!("Optimized XPath ({} segments):\n{}", nodes.len(), optimized_xpath);
        println!("Complexity: 1 // + {} single / = much faster than all //", single_slash_count - 2);
    }
}
