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

    /// Build the XPath segment for this node (e.g. `//Button[@AutomationId='x']`).
    pub fn xpath_segment(&self) -> String {
        let predicates: Vec<String> = self.filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();

        if predicates.is_empty() {
            format!("//{}", self.tag())
        } else {
            format!("//{}[{}]", self.tag(), predicates.join(" and "))
        }
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

// ─── CaptureResult ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub hierarchy:  Vec<HierarchyNode>,
    pub cursor_x:   i32,
    pub cursor_y:   i32,
    pub error:      Option<String>,
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
