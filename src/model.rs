// src/model.rs
use serde::{Deserialize, Serialize};

// ─── Operator ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
}

impl Operator {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Equals     => "等于",
            Self::NotEquals  => "不等于",
            Self::Contains   => "包含",
            Self::StartsWith => "开头为",
            Self::EndsWith   => "结尾为",
        }
    }

    pub fn all() -> &'static [Operator] {
        &[
            Operator::Equals,
            Operator::NotEquals,
            Operator::Contains,
            Operator::StartsWith,
            Operator::EndsWith,
        ]
    }

    /// Generate the XPath predicate fragment for this operator.
    pub fn to_predicate(&self, attr: &str, value: &str) -> String {
        match self {
            Self::Equals     => format!("@{}='{}'", attr, value),
            Self::NotEquals  => format!("@{}!='{}'", attr, value),
            Self::Contains   => format!("contains(@{}, '{}')", attr, value),
            Self::StartsWith => format!("starts-with(@{}, '{}')", attr, value),
            Self::EndsWith   => format!(
                "substring(@{0}, string-length(@{0})-{1}+1)='{2}'",
                attr,
                value.len(),
                value
            ),
        }
    }
}

// ─── PropertyFilter ──────────────────────────────────────────────────────────

/// A single attribute match condition within a hierarchy node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyFilter {
    pub name:     String,
    pub operator: Operator,
    pub value:    String,
    pub enabled:  bool,
}

impl PropertyFilter {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        let enabled = !value.is_empty();
        Self { name: name.into(), operator: Operator::Equals, value, enabled }
    }

    /// Returns the XPath predicate string, or empty string if disabled/empty.
    pub fn predicate(&self) -> Option<String> {
        if !self.enabled || self.value.is_empty() {
            return None;
        }
        Some(self.operator.to_predicate(&self.name, &self.value))
    }
}

// ─── ElementRect ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementRect {
    pub x:      i32,
    pub y:      i32,
    pub width:  i32,
    pub height: i32,
}

// ─── HierarchyNode ───────────────────────────────────────────────────────────

/// One level in the ancestor chain from root window to target element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub control_type:   String,
    pub automation_id:  String,
    pub class_name:     String,
    pub name:           String,
    pub index:          i32,         // 1-based sibling index, 0 = unknown
    pub rect:           ElementRect,
    pub process_id:     u32,
    pub filters:        Vec<PropertyFilter>,
    /// Whether this node should be included in the final XPath.
    pub included:       bool,
}

impl HierarchyNode {
    pub fn new(
        control_type:  impl Into<String>,
        automation_id: impl Into<String>,
        class_name:    impl Into<String>,
        name:          impl Into<String>,
        index:         i32,
        rect:          ElementRect,
        process_id:    u32,
    ) -> Self {
        let ct  = control_type.into();
        let aid = automation_id.into();
        let cn  = class_name.into();
        let nm  = name.into();

        let filters = vec![
            PropertyFilter::new("AutomationId", &aid),
            PropertyFilter::new("ClassName",    &cn),
            PropertyFilter::new("Name",         &nm),
            PropertyFilter::new("Index",        if index > 0 { index.to_string() } else { String::new() }),
        ];

        Self {
            control_type: ct,
            automation_id: aid,
            class_name: cn,
            name: nm,
            index,
            rect,
            process_id,
            filters,
            included: true,
        }
    }

    /// XPath tag name derived from ControlType.
    pub fn tag(&self) -> &str {
        if self.control_type.is_empty() { "*" } else { &self.control_type }
    }

    /// Build the XPath segment for this node.
    /// The first node in the hierarchy uses "//" (search from root).
    /// Subsequent nodes use "/" (direct children only) for better performance.
    pub fn xpath_segment(&self) -> String {
        let predicates: Vec<String> = self.filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();

        // Note: The prefix (// or /) is determined by the caller based on position.
        // This function only builds the tag and predicates part.
        if predicates.is_empty() {
            format!("{}", self.tag())
        } else {
            format!("{}[{}]", self.tag(), predicates.join(" and "))
        }
    }

    /// Build the full XPath segment with proper prefix.
    /// is_first: true for the root node (//), false for children (/)
    pub fn xpath_segment_with_prefix(&self, is_first: bool) -> String {
        let prefix = if is_first { "//" } else { "/" };
        let segment = self.xpath_segment();
        format!("{}{}", prefix, segment)
    }

    /// Short label shown in the hierarchy tree panel.
    pub fn tree_label(&self) -> String {
        let mut parts = vec![self.control_type.clone()];
        if !self.automation_id.is_empty() {
            parts.push(format!("id=\"{}\"", truncate(&self.automation_id, 22)));
        } else if !self.name.is_empty() {
            parts.push(format!("name=\"{}\"", truncate(&self.name, 22)));
        } else if !self.class_name.is_empty() {
            parts.push(format!("class=\"{}\"", truncate(&self.class_name, 22)));
        }
        if self.index > 0 {
            parts.push(format!("[{}]", self.index));
        }
        parts.join("  ")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

// ─── WindowInfo ──────────────────────────────────────────────────────────────

/// Information about the target window for fast XPath validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowInfo {
    pub title: String,           // Window title
    pub class_name: String,      // Window class name
    pub process_id: u32,         // Process ID
}

// ─── CaptureResult ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub hierarchy:  Vec<HierarchyNode>,
    pub cursor_x:   i32,
    pub cursor_y:   i32,
    pub error:      Option<String>,
    /// Window information extracted from hierarchy (for fast validation).
    pub window_info: Option<WindowInfo>,
}

// ─── ValidationResult ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Idle,
    Running,
    Found { count: usize, first_rect: Option<ElementRect> },
    NotFound,
    Error(String),
}

impl ValidationResult {
    pub fn label(&self) -> String {
        match self {
            Self::Idle       => "校验元素  F7".into(),
            Self::Running    => "校验中…".into(),
            Self::Found { count, .. } => format!("✔ 找到 {} 个", count),
            Self::NotFound   => "✘ 未找到".into(),
            Self::Error(e)   => format!("⚠ {}", e),
        }
    }
}

// ─── AppConfig (persisted) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub highlight_on_hover: bool,
    pub show_simplified:    bool,
    pub last_xpaths:        Vec<String>,   // history, newest first
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            highlight_on_hover: true,
            show_simplified:    false,
            last_xpaths:        Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_info_extraction() {
        // Test that WindowInfo can be created and compared
        let info1 = WindowInfo {
            title: "微信".to_string(),
            class_name: "mmui::MainWindow".to_string(),
            process_id: 12345,
        };
        
        let info2 = WindowInfo {
            title: "微信".to_string(),
            class_name: "mmui::MainWindow".to_string(),
            process_id: 12345,
        };
        
        assert_eq!(info1, info2, "WindowInfo should support equality comparison");
    }

    #[test]
    fn test_capture_result_with_window_info() {
        let result = CaptureResult {
            hierarchy: vec![
                HierarchyNode::new("Window", "", "Notepad", "Untitled", 0, ElementRect::default(), 1234),
                HierarchyNode::new("Edit", "", "Edit", "", 0, ElementRect::default(), 1234),
            ],
            cursor_x: 100,
            cursor_y: 200,
            error: None,
            window_info: Some(WindowInfo {
                title: "Untitled".to_string(),
                class_name: "Notepad".to_string(),
                process_id: 1234,
            }),
        };
        
        assert!(result.window_info.is_some());
        let win = result.window_info.unwrap();
        assert_eq!(win.title, "Untitled");
        assert_eq!(win.class_name, "Notepad");
        assert_eq!(win.process_id, 1234);
    }

    #[test]
    fn test_capture_result_without_window_info() {
        // Desktop capture might not have Window in hierarchy
        let result = CaptureResult {
            hierarchy: vec![
                HierarchyNode::new("Pane", "", "#32769", "桌面", 0, ElementRect::default(), 0),
            ],
            cursor_x: 500,
            cursor_y: 300,
            error: None,
            window_info: None,
        };
        
        assert!(result.window_info.is_none());
    }
}
