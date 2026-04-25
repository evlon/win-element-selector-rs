// src/xpath.rs
use crate::model::HierarchyNode;

/// Build the full XPath from the active (included) nodes.
pub fn generate(nodes: &[HierarchyNode]) -> String {
    nodes
        .iter()
        .filter(|n| n.included)
        .map(|n| n.xpath_segment())
        .collect::<Vec<_>>()
        .join("")
}

/// Attempt to produce a shorter XPath by keeping only nodes with
/// a unique-enough identity (AutomationId or Name set), plus the target.
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
        .map(|n| n.xpath_segment())
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
        assert!(x.contains("//Window"));
        assert!(x.contains("AutomationId='main'"));
        assert!(x.contains("//Button"));
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
        assert!(x.contains("//Button"));
    }

    #[test]
    fn lint_catches_imbalance() {
        assert!(lint("//Button[@AutomationId='x'").is_some());
        assert!(lint("//Button[@AutomationId='x']").is_none());
        assert!(lint("").is_some());
        assert!(lint("Button[@id='x']").is_some());
    }
}
