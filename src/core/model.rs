// src/core/model.rs
//
// Core data models shared between GUI and HTTP API.

use uiauto_xpath::{is_dynamic_class, extract_stable_prefix};
use serde::{Deserialize, Serialize};

// ─── CaptureMode ─────────────────────────────────────────────────────────────

/// 捕获模式：决定使用哪种 UIA TreeWalker 和策略
///
/// - `Fast`：性能极致，只用 ControlViewWalker。适合大多数原生应用（Qt、Win32、WPF）
/// - `Full`：增强捕获，RawViewWalker + 子进程窗口 + 缓存。能捕获所有元素（包括 WebView/Chrome 嵌入），可接受慢一些
/// - `FastChild`：目标在子窗口内，从子窗口 Root 开始查找。跳过主窗口 UI 树遍历。XPath 前缀 `[fast-child]`
/// - `FullChild`：增强捕获 + 子窗口。XPath 前缀 `[full-child]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMode {
    /// 性能极致模式：只用 ControlViewWalker，XPath 前缀 `[fast]`
    Fast,
    /// 增强捕获模式：RawViewWalker + 子进程窗口 + 缓存，XPath 前缀 `[full]`
    Full,
    /// 快速捕获 + 子窗口：目标在子 HWND 内，XPath 前缀 `[fast-child]`
    FastChild,
    /// 增强捕获 + 子窗口：目标在子 HWND 内，XPath 前缀 `[full-child]`
    FullChild,
}

impl CaptureMode {
    /// XPath 前缀字符串
    pub fn xpath_prefix(&self) -> &'static str {
        match self {
            CaptureMode::Fast => "[fast]",
            CaptureMode::Full => "[full]",
            CaptureMode::FastChild => "[fast-child]",
            CaptureMode::FullChild => "[full-child]",
        }
    }

    /// 是否是子窗口模式
    pub fn is_child_mode(&self) -> bool {
        matches!(self, CaptureMode::FastChild | CaptureMode::FullChild)
    }

    /// 从 XPath 前缀解析 CaptureMode
    /// 返回 None 表示没有前缀（向后兼容，走完整 fallback）
    pub fn from_xpath_prefix(xpath: &str) -> Option<CaptureMode> {
        if xpath.starts_with("[fast-child]") {
            Some(CaptureMode::FastChild)
        } else if xpath.starts_with("[full-child]") {
            Some(CaptureMode::FullChild)
        } else if xpath.starts_with("[fast]") {
            Some(CaptureMode::Fast)
        } else if xpath.starts_with("[full]") {
            Some(CaptureMode::Full)
        } else {
            None
        }
    }

    /// 剥离 XPath 前缀，返回 (capture_mode, stripped_xpath)
    /// 如果没有前缀，返回 (None, original_xpath)
    pub fn strip_xpath_prefix(xpath: &str) -> (Option<CaptureMode>, &str) {
        if let Some(rest) = xpath.strip_prefix("[fast-child]") {
            (Some(CaptureMode::FastChild), rest)
        } else if let Some(rest) = xpath.strip_prefix("[full-child]") {
            (Some(CaptureMode::FullChild), rest)
        } else if let Some(rest) = xpath.strip_prefix("[fast]") {
            (Some(CaptureMode::Fast), rest)
        } else if let Some(rest) = xpath.strip_prefix("[full]") {
            (Some(CaptureMode::Full), rest)
        } else {
            (None, xpath)
        }
    }
}

impl std::fmt::Display for CaptureMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureMode::Fast => write!(f, "快速捕获"),
            CaptureMode::Full => write!(f, "增强捕获"),
            CaptureMode::FastChild => write!(f, "快速捕获(子窗口)"),
            CaptureMode::FullChild => write!(f, "增强捕获(子窗口)"),
        }
    }
}

// ─── Operator ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operator {
    Equals,           // 等于: @Name='value'
    NotEquals,        // 不等于: @Name!='value'
    Contains,         // 包含: contains(@Name, 'value')
    NotContains,      // 不包含: not(contains(@Name, 'value'))
    StartsWith,       // 开头为: starts-with(@Name, 'value')
    NotStartsWith,    // 开头不为: not(starts-with(@Name, 'value'))
    EndsWith,         // 结尾为: substring(@Name, ...)='value'
    NotEndsWith,      // 结尾不为: not(substring(@Name, ...)='value')
    Matches,          // 正则匹配: matches(@Name, 'pattern')
    NotMatches,       // 正则不匹配: not(matches(@Name, 'pattern'))
    GreaterThan,      // 大于: @Index > value (numeric)
    GreaterThanOrEq,  // 大于等于: @Index >= value
    LessThan,         // 小于: @Index < value
    LessThanOrEq,     // 小于等于: @Index <= value
}

impl Operator {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Equals          => "等于",
            Self::NotEquals       => "不等于",
            Self::Contains        => "包含",
            Self::NotContains     => "不包含",
            Self::StartsWith      => "开头为",
            Self::NotStartsWith   => "开头不为",
            Self::EndsWith        => "结尾为",
            Self::NotEndsWith     => "结尾不为",
            Self::Matches         => "正则匹配",
            Self::NotMatches      => "正则不匹配",
            Self::GreaterThan     => "大于",
            Self::GreaterThanOrEq => "大于等于",
            Self::LessThan        => "小于",
            Self::LessThanOrEq    => "小于等于",
        }
    }

    pub fn all() -> &'static [Operator] {
        &[
            Operator::Equals,
            Operator::NotEquals,
            Operator::Contains,
            Operator::NotContains,
            Operator::StartsWith,
            Operator::NotStartsWith,
            Operator::EndsWith,
            Operator::NotEndsWith,
            Operator::Matches,
            Operator::NotMatches,
            Operator::GreaterThan,
            Operator::GreaterThanOrEq,
            Operator::LessThan,
            Operator::LessThanOrEq,
        ]
    }

    /// Generate the XPath predicate fragment for this operator.
    pub fn to_predicate(&self, attr: &str, value: &str) -> String {
        match self {
            Self::Equals          => format!("@{}='{}'", attr, value),
            Self::NotEquals       => format!("@{}!='{}'", attr, value),
            Self::Contains        => format!("contains(@{}, '{}')", attr, value),
            Self::NotContains     => format!("not(contains(@{}, '{}'))", attr, value),
            Self::StartsWith      => format!("starts-with(@{}, '{}')", attr, value),
            Self::NotStartsWith   => format!("not(starts-with(@{}, '{}'))", attr, value),
            Self::EndsWith        => {
                let val_len = value.chars().count();
                format!("substring(@{0}, string-length(@{0})-{1}+1)='{2}'", attr, val_len, value)
            }
            Self::NotEndsWith     => {
                let val_len = value.chars().count();
                format!("not(substring(@{0}, string-length(@{0})-{1}+1)='{2}')", attr, val_len, value)
            }
            Self::Matches         => format!("matches(@{}, '{}')", attr, value),
            Self::NotMatches      => format!("not(matches(@{}, '{}'))", attr, value),
            // Numeric comparisons (for Index, etc.)
            Self::GreaterThan     => format!("@{} > {}", attr, value),
            Self::GreaterThanOrEq => format!("@{} >= {}", attr, value),
            Self::LessThan        => format!("@{} < {}", attr, value),
            Self::LessThanOrEq    => format!("@{} <= {}", attr, value),
        }
    }
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ─── WalkerHint ──────────────────────────────────────────────────────────────

/// 提示校验时应该使用哪种 UIA TreeWalker 来查找该节点的子节点。
/// 
/// 捕获元素时，我们知道 hierarchy 中每一层在 UIA 树中的实际位置特征。
/// 将此信息记录到 XPath/HierarchyNode 中，可以避免校验时盲目尝试所有 fallback 策略。
/// 
/// **场景说明**：
/// - 微信主窗口 `mmui::MainWindow` 的子节点在 ControlView 中可见 → ControlView
/// - 微信内嵌 `Chrome_WidgetWin_0` 子窗口的子节点在 RawView 中才可见 → RawView  
/// - 跨进程 WebView 子窗口的元素需要通过 EnumChildWindows 找到 → ChildHwnd
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalkerHint {
    /// 默认：使用 ControlViewWalker（uiauto-xpath），最快
    /// 适用于大多数原生控件（Qt、Win32、WPF 等）
    ControlView,
    /// 使用 RawViewWalker（tree walk），稍慢但能看到 ControlView 过滤掉的元素
    /// 适用于 Chrome/WebView 嵌入场景
    RawView,
    /// 需要通过 EnumChildWindows 找到子 HWND，然后在其子树中搜索
    /// 适用于跨进程 WebView（如微信的 WeChatAppEx）
    ChildHwnd,
    /// 未知/未设置：使用默认 fallback 策略
    Unknown,
}

impl Default for WalkerHint {
    fn default() -> Self {
        WalkerHint::Unknown
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

    /// Create filter with custom operator (e.g., StartsWith for dynamic class names)
    pub fn new_with_operator(name: impl Into<String>, value: impl Into<String>, operator: Operator) -> Self {
        let value = value.into();
        // NotEquals with empty value means "attribute is not empty" — meaningful, keep enabled
        let enabled = !value.is_empty() || matches!(operator, Operator::NotEquals | Operator::Equals);
        Self { name: name.into(), operator, value, enabled }
    }

    /// Create disabled filter (will be skipped in XPath generation)
    pub fn new_disabled(name: impl Into<String>) -> Self {
        Self { name: name.into(), operator: Operator::Equals, value: String::new(), enabled: false }
    }

    /// Returns the XPath predicate string, or empty string if disabled/empty.
    pub fn predicate(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        // Equals/NotEquals with empty value: compare against empty string
        if self.value.is_empty() {
            return match self.operator {
                Operator::Equals    => Some(format!("@{}=''", self.name)),
                Operator::NotEquals => Some(format!("@{}!=''", self.name)),
                _ => None, // Contains/StartsWith etc. with empty value are meaningless
            };
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

impl ElementRect {
    /// 计算与另一个矩形的重叠面积占比
    /// 返回 Some(overlap_ratio) 如果重叠，None 如果不重叠
    pub fn overlap_ratio(&self, other: &ElementRect) -> Option<f64> {
        // 计算重叠区域
        let overlap_x = (self.x + self.width).min(other.x + other.width)
                      - self.x.max(other.x);
        let overlap_y = (self.y + self.height).min(other.y + other.height)
                      - self.y.max(other.y);
        
        // 如果没有重叠，返回 None
        if overlap_x <= 0 || overlap_y <= 0 {
            return None;
        }
        
        // 计算重叠面积
        let overlap_area = (overlap_x * overlap_y) as f64;
        let area_self = (self.width * self.height) as f64;
        let area_other = (other.width * other.height) as f64;
        
        // 使用较小面积的作为基准
        let min_area = area_self.min(area_other);
        
        if min_area <= 0.0 {
            return None;
        }
        
        Some(overlap_area / min_area)
    }
    
    /// 判断两个矩形是否"视觉重叠"（重叠面积 > 80%）
    pub fn is_visually_overlapping(&self, other: &ElementRect) -> bool {
        match self.overlap_ratio(other) {
            Some(ratio) => ratio > 0.8,
            None => false,
        }
    }
}

// ─── ChildPredicate ──────────────────────────────────────────────────────────

/// 子元素谓词：描述当前节点下的一个子元素特征。
/// 用于生成 `Button[Text[@Name='确认']]` 这样的嵌套谓词。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildPredicate {
    /// 子元素的 ControlType（如 "Text"）
    pub control_type: String,
    /// 子元素的属性过滤条件
    pub filters: Vec<PropertyFilter>,
}

impl ChildPredicate {
    /// 构建 XPath 子谓词字符串，如 `Text[@Name='确认']`
    pub fn to_xpath(&self) -> String {
        let preds: Vec<String> = self.filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();
        if preds.is_empty() {
            format!("{}", self.control_type)
        } else {
            format!("{}[{}]", self.control_type, preds.join(" and "))
        }
    }
}

// ─── HierarchyNode ───────────────────────────────────────────────────────────

/// One level in the ancestor chain from root window to target element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub control_type:          String,
    pub automation_id:         String,
    pub class_name:            String,
    pub name:                  String,
    pub index:                 i32,         // 1-based sibling index, 0 = unknown
    pub framework_id:          String,      // FrameworkId: "Win32", "WPF", "Qt", "WinForms", "UWP"
    pub acc_role:              String,      // Accessibility Role
    pub help_text:             String,      // HelpText property
    pub localized_control_type: String,     // LocalizedControlType
    pub is_enabled:            bool,        // IsEnabled property
    pub is_offscreen:          bool,        // IsOffscreen property
    pub is_password:           bool,        // IsPassword property
    #[serde(default)]
    pub accelerator_key:       String,      // AcceleratorKey property
    #[serde(default)]
    pub access_key:            String,      // AccessKey property
    #[serde(default)]
    pub item_type:             String,      // ItemType property
    #[serde(default)]
    pub item_status:           String,      // ItemStatus property
    pub rect:                  ElementRect,
    pub process_id:            u32,
    pub filters:               Vec<PropertyFilter>,
    /// Whether this node should be included in the final XPath.
    pub included:              bool,
    /// Whether this node is the target element (the one user captured).
    /// Used by optimizer to correctly identify the target node.
    pub is_target:             bool,
    /// Position function mode for Index:
    /// - "position": position()=N (default)
    /// - "first": first() (when position=1)
    /// - "last": last() (when position is last sibling)
    /// - "index": @Index='N' (fallback)
    pub position_mode:         String,
    /// Total sibling count (needed for last() detection)
    pub sibling_count:         i32,
    /// 该节点在真实UIA树中距离窗口根节点的层级深度。
    /// 用于判断XPath前缀：
    /// - depth差值=1 → 父子关系，用 `/`
    /// - depth差值>1 → 跳过中间层，用 `//`
    /// 窗口节点本身 depth=0，其直接子节点 depth=1，以此类推。
    #[serde(default)]
    pub depth_from_window:     usize,
    /// 提示校验时应该使用哪种 UIA TreeWalker 来查找该节点的子节点。
    /// 捕获时自动记录，校验时优先使用此 hint 避免不必要的 fallback 尝试。
    /// - ControlView: 用 find_by_xpath_detailed（uiauto-xpath）最快
    /// - RawView: 用 RawViewWalker BFS 遍历
    /// - ChildHwnd: 需要先 EnumChildWindows 找到子 HWND
    /// - Unknown: 使用完整 fallback 策略
    #[serde(default)]
    pub walker_hint:           WalkerHint,
    /// 子元素谓词：捕获时为 target 节点收集的有意义子元素特征。
    /// 用于生成 `Button[Text[@Name='确认']]` 这种更精确的 XPath。
    /// 仅在 target 节点上非空。
    #[serde(default)]
    pub child_predicates:      Vec<ChildPredicate>,
    // ─── UIA Pattern availability (for element state detection) ─────────────
    #[serde(default)]
    pub is_checkable:          bool,        // TogglePattern available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_checked:            Option<bool>, // ToggleState
    #[serde(default)]
    pub is_clickable:          bool,        // InvokePattern available or clickable ControlType
    #[serde(default)]
    pub is_scrollable:         bool,        // ScrollPattern available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_selected:           Option<bool>, // SelectionItemPattern IsSelected
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

        // Build filters with extended properties
        // ClassName: detect dynamic class names and use stable prefix
        let class_filter = if is_dynamic_class(&cn) {
            let prefix = extract_stable_prefix(&cn);
            if prefix.len() >= 4 {
                PropertyFilter::new_with_operator("ClassName", prefix, Operator::StartsWith)
            } else {
                // Prefix too short, disable ClassName filter
                PropertyFilter::new_disabled("ClassName")
            }
        } else {
            PropertyFilter::new("ClassName", &cn)
        };
        
        // Note: ControlType is NOT added as a filter because it's already expressed by the XPath tag name.
        // The tag() method returns control_type, so //Button[@Name='...'] implicitly means ControlType='Button'.
        let mut filters = vec![
            PropertyFilter::new("AutomationId",   &aid),
            class_filter,
            PropertyFilter::new("Name",           &nm),
        ];
        
        // Add Index filter (will use position() function if use_position_function is true)
        if index > 0 {
            filters.push(PropertyFilter::new("Index", index.to_string()));
        }

        Self {
            control_type: ct,
            automation_id: aid,
            class_name: cn,
            name: nm,
            index,
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
            rect,
            process_id,
            filters,
            included: true,
            is_target: false,          // 默认非目标节点，捕获时会设置最后一个节点为目标
            position_mode: "position".to_string(),  // Default to position()=N
            sibling_count: 0,  // Will be computed during capture
            depth_from_window: 0,  // 默认值，捕获时会计算真实深度
            walker_hint: WalkerHint::Unknown,  // 默认值，捕获时会根据实际遍历方式设置
            child_predicates: Vec::new(),  // 默认空，捕获时为目标节点填充
            is_checkable: false,
            is_checked: None,
            is_clickable: false,
            is_scrollable: false,
            is_selected: None,
        }
    }

    /// 构建扩展属性过滤器
    /// 在 UIA 捕获阶段填充完扩展字段后调用，将所有有区分度的属性加入 filters
    /// 策略：
    /// - 字符串属性：非空时添加（如 HelpText, AcceleratorKey 等）
    /// - 布尔属性：特殊值时添加（如 IsPassword=true, IsEnabled=false）
    /// - IsOffscreen 不添加：Chrome/WebView 的中间容器 Group 通常标记为 true，
    ///   但该属性在不同 UIA 上下文（Walker/树范围）中返回值不稳定，会导致校验失败
    pub fn build_extended_filters(&mut self) {
        // 字符串属性：非空时添加
        if !self.framework_id.is_empty() {
            self.filters.push(PropertyFilter::new("FrameworkId", &self.framework_id));
        }
        if !self.help_text.is_empty() {
            self.filters.push(PropertyFilter::new("HelpText", &self.help_text));
        }
        // LocalizedControlType is system-language dependent, skip to keep XPath portable
        if !self.accelerator_key.is_empty() {
            self.filters.push(PropertyFilter::new("AcceleratorKey", &self.accelerator_key));
        }
        if !self.access_key.is_empty() {
            self.filters.push(PropertyFilter::new("AccessKey", &self.access_key));
        }
        if !self.item_type.is_empty() {
            self.filters.push(PropertyFilter::new("ItemType", &self.item_type));
        }
        if !self.item_status.is_empty() {
            self.filters.push(PropertyFilter::new("ItemStatus", &self.item_status));
        }
        // 布尔属性：特殊值时添加（有区分度）
        if self.is_password {
            self.filters.push(PropertyFilter::new("IsPassword", "true"));
        }
        if !self.is_enabled {
            self.filters.push(PropertyFilter::new("IsEnabled", "false"));
        }
        // IsOffscreen 不生成过滤器，原因见上方注释
    }

    /// XPath tag name derived from ControlType.
    pub fn tag(&self) -> &str {
        if self.control_type.is_empty() { "*" } else { &self.control_type }
    }

    /// Build the XPath segment for this node.
    /// Supports position(), first(), last() functions for better XPath standard compatibility.
    /// Also supports child element predicates like `Button[Text[@Name='确认']]`.
    pub fn xpath_segment(&self) -> String {
        let predicates: Vec<String> = self.filters
            .iter()
            .filter_map(|f| {
                // Special handling for Index: convert to position() function
                if f.name == "Index" && f.enabled && !f.value.is_empty() {
                    if let Ok(pos) = f.value.parse::<i32>() {
                        if pos > 0 {
                            return Some(self.format_position(pos));
                        }
                    }
                }
                // Regular property filter
                f.predicate()
            })
            .chain(
                // Append child predicates: Text[@Name='xxx'], etc.
                self.child_predicates.iter().map(|cp| cp.to_xpath())
            )
            .collect();

        // Note: The prefix (// or /) is determined by the caller based on position.
        // This function only builds the tag and predicates part.
        if predicates.is_empty() {
            format!("{}", self.tag())
        } else {
            format!("{}[{}]", self.tag(), predicates.join(" and "))
        }
    }

    /// Format position using XPath functions: first(), last(), or position()=N
    fn format_position(&self, pos: i32) -> String {
        match self.position_mode.as_str() {
            "first" => {
                // Always use first() (user override)
                "first()".to_string()
            }
            "last" => {
                // Always use last() (user override)
                "last()".to_string()
            }
            "index" => {
                // Fallback to @Index attribute
                format!("@Index='{}'", pos)
            }
            _ => {
                // Default: "position" mode - use smart detection
                if pos == 1 {
                    // First element: use first()
                    "first()".to_string()
                } else if self.sibling_count > 0 && pos == self.sibling_count {
                    // Last element: use last()
                    "last()".to_string()
                } else {
                    // Middle element: use position()=N
                    format!("position()={}", pos)
                }
            }
        }
    }

    /// Build the full XPath segment with proper prefix.
    /// is_first: true for the root node (//), false for children (/)
    #[allow(dead_code)]
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
    pub process_name: String,    // Process name (e.g., 'Weixin', 'chrome')
}

// ─── XPathResult ─────────────────────────────────────────────────────────────

/// Complete XPath selection result: window selector + element XPath.
pub struct XPathResult {
    /// Window selector in XPath-like format, e.g. "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    pub window_selector: String,
    /// Element XPath starting from window root, with capture mode prefix.
    /// e.g. "[fast]/Group/Button[@Name='发送']" or "[full]/Group//Custom/..."
    pub element_xpath: String,
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
    /// 捕获模式：标识此捕获结果是用哪种策略生成的
    pub capture_mode: CaptureMode,
}

// ─── ValidationResult ────────────────────────────────────────────────────────

/// 单个属性校验结果（用于逐层校验显示）
#[derive(Debug, Clone)]
pub struct PropertyValidationResult {
    /// 属性名 (如 ClassName, Name, ControlType)
    pub attr_name: String,
    /// 运算符 (Equals, StartsWith, Contains 等)
    pub operator: Operator,
    /// XPath 中的期望值
    pub expected_value: String,
    /// 实际元素的值（校验后获取）
    pub actual_value: Option<String>,
    /// 匹配状态: true = 匹配成功, false = 不匹配
    pub matched: bool,
    /// 此属性是否启用（参与校验）
    pub enabled: bool,
}

/// 单个层级节点的校验结果
#[derive(Debug, Clone)]
pub struct LayerValidationResult {
    /// 节点在 hierarchy 中的索引
    pub node_index: usize,
    /// ControlType (用于显示)
    pub control_type: String,
    /// 节点简短描述（用于UI显示）
    pub node_label: String,
    /// 是否匹配（此层级是否找到元素）
    pub matched: bool,
    /// 各属性的校验结果
    pub properties: Vec<PropertyValidationResult>,
    /// 匹配的元素数量
    pub match_count: usize,
    /// 此层级执行时间 (ms)
    pub duration_ms: u64,
}

/// 单个属性校验失败详情（用于错误提示）
#[derive(Debug, Clone)]
pub struct PredicateFailure {
    /// 属性名 (如 ClassName, Name)
    pub attr_name: String,
    /// XPath 中的期望值
    pub expected_value: String,
    /// 实际元素的值（如果能获取）
    pub actual_value: Option<String>,
    /// 失败原因提示
    pub reason: String,
}

/// Result of validating a single XPath segment.
#[derive(Debug, Clone)]
pub struct SegmentValidationResult {
    /// Segment index (0-based)
    pub segment_index: usize,
    /// Segment text (e.g., "/Group[@ClassName='QWidget']")
    pub segment_text: String,
    /// Whether this segment matched
    pub matched: bool,
    /// Number of matches found at this level
    pub match_count: usize,
    /// Time taken to validate this segment (in milliseconds)
    pub duration_ms: u64,
    /// 失败的 predicate 详情（如果匹配失败）
    pub predicate_failures: Vec<PredicateFailure>,
}

/// Detailed validation result with per-segment information.
#[derive(Debug, Clone)]
pub struct DetailedValidationResult {
    /// Overall result
    pub overall: ValidationResult,
    /// Per-segment validation results (XPath step results)
    pub segments: Vec<SegmentValidationResult>,
    /// Per-layer validation results (hierarchy node results) - 新增
    pub layers: Vec<LayerValidationResult>,
    /// Total validation time (in milliseconds)
    pub total_duration_ms: u64,
    /// 第一个匹配元素是否在屏幕外
    pub is_offscreen: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Idle,
    Running,
    Found { count: usize, first_rect: Option<ElementRect>, rects: Vec<ElementRect> },
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
            Self::Error(_)   => "⚠ 校验错误".into(),
        }
    }
    
    /// 获取详细错误消息（用于状态栏显示）
    pub fn error_message(&self) -> Option<String> {
        match self {
            Self::Error(e) => Some(e.clone()),
            _ => None,
        }
    }
}

// ─── ElementTab (标签页) ────────────────────────────────────────────────────

/// 标签页类型：元素定位或窗口元素定位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementTab {
    Element,      // 元素定位（默认）
    WindowElement, // 窗口元素定位
}

impl Default for ElementTab {
    fn default() -> Self {
        ElementTab::Element
    }
}

// ─── HighlightInfo ──────────────────────────────────────────────────────────

/// Information needed for highlight window display.
#[derive(Debug, Clone)]
pub struct HighlightInfo {
    /// Element rectangle for positioning
    pub rect: ElementRect,
    /// Element control type (English, e.g., "Button", "Edit")
    pub control_type: String,
    /// Element control type in Chinese (e.g., "按钮", "可编辑文本")
    pub control_type_cn: String,
}

impl HighlightInfo {
    pub fn new(rect: ElementRect, control_type: &str) -> Self {
        Self {
            rect,
            control_type: control_type.to_string(),
            control_type_cn: control_type_to_chinese(control_type),
        }
    }
}

/// Convert English control type to Chinese description.
pub fn control_type_to_chinese(control_type: &str) -> String {
    match control_type {
        "Button"       => "按钮",
        "Calendar"     => "日历",
        "CheckBox"     => "复选框",
        "ComboBox"     => "下拉框",
        "Custom"       => "自定义控件",
        "DataGrid"     => "数据网格",
        "DataItem"     => "数据项",
        "Document"     => "文档",
        "Edit"         => "可编辑文本",
        "Group"        => "分组",
        "Header"       => "表头",
        "HeaderItem"   => "表头项",
        "Hyperlink"    => "超链接",
        "Image"        => "图片",
        "List"         => "列表",
        "ListItem"     => "列表项",
        "MenuBar"      => "菜单栏",
        "Menu"         => "菜单",
        "MenuItem"     => "菜单项",
        "Pane"         => "面板",
        "ProgressBar"  => "进度条",
        "RadioButton"  => "单选按钮",
        "ScrollBar"    => "滚动条",
        "SemanticZoom" => "语义缩放",
        "Separator"    => "分隔符",
        "Slider"       => "滑块",
        "Spinner"      => "数值调节钮",
        "SplitButton"  => "拆分按钮",
        "StatusBar"    => "状态栏",
        "Tab"          => "选项卡",
        "TabItem"      => "选项卡项",
        "Table"        => "表格",
        "Text"         => "文本",
        "Thumb"        => "缩略图",
        "TitleBar"     => "标题栏",
        "ToolBar"      => "工具栏",
        "ToolTip"      => "提示",
        "Tree"         => "树形控件",
        "TreeItem"     => "树项",
        "Window"       => "窗口",
        "Desktop"      => "桌面",
        "Application"  => "应用程序",
        _               => control_type,  // Return original if no translation
    }.to_string()
}

// ─── HistoryEntry ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub name: String,
    pub xpath_text: String,
    pub window_title: String,
    pub control_type: String,
    pub timestamp: u64,
}

impl HistoryEntry {
    pub fn from_capture(
        xpath_text: &str,
        window_info: Option<&WindowInfo>,
        hierarchy: &[HierarchyNode],
    ) -> Self {
        let window_title = window_info
            .map(|w| w.title.clone())
            .or_else(|| hierarchy.first().map(|n| n.name.clone()))
            .unwrap_or_default();
        let control_type = hierarchy
            .last()
            .map(|n| n.control_type.clone())
            .unwrap_or_default();
        // 优先用 hierarchy 中的 name，否则从 XPath 中提取最后一个 @Name
        // xpath_text 格式: "window_selector, element_xpath"
        let element_xpath = xpath_text.splitn(2, ", ").nth(1).unwrap_or(xpath_text);
        let element_name = match hierarchy.last() {
            Some(node) if !node.name.is_empty() => node.name.clone(),
            _ => extract_xpath_name(element_xpath).unwrap_or_default(),
        };
        // 优先用元素名称生成描述性名称，如 "搜一搜 (Button)"；否则用 "Button in 微信"
        let name = if !element_name.is_empty() {
            format!("{element_name} ({control_type})")
        } else if !window_title.is_empty() {
            format!("{control_type} in {window_title}")
        } else {
            control_type.clone()
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name,
            xpath_text: xpath_text.to_string(),
            window_title,
            control_type,
            timestamp,
        }
    }

    pub fn display_time(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(self.timestamp);
        let elapsed = now.saturating_sub(self.timestamp);
        if elapsed < 60 {
            "刚刚".to_string()
        } else if elapsed < 3600 {
            format!("{}分钟前", elapsed / 60)
        } else if elapsed < 86400 {
            format!("{}小时前", elapsed / 3600)
        } else if elapsed < 604800 {
            format!("{}天前", elapsed / 86400)
        } else {
            format!("{}周前", elapsed / 604800)
        }
    }

    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let haystack = format!(
            "{} {} {} {}",
            self.name, self.window_title, self.control_type, self.xpath_text
        )
        .to_lowercase();
        haystack.contains(&query.to_lowercase())
    }
}

/// 从 element XPath 中提取最后一个 @Name 值，用于命名
/// 例如: "//Group/Button[@Name='搜一搜']/Group/Button" → "搜一搜"
/// 会倒序扫描所有段，找第一个带 @Name 的段
fn extract_xpath_name(xpath: &str) -> Option<String> {
    for segment in xpath.rsplit('/') {
        if segment.is_empty() { continue; }
        for quote in ['\'', '"'] {
            let marker = &format!("@Name={}", quote);
            if let Some(start) = segment.rfind(marker) {
                let start = start + marker.len();
                if let Some(end) = segment[start..].find(quote) {
                    let name = &segment[start..start + end];
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

// ─── AppConfig (persisted) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub history: Vec<HistoryEntry>,   // 历史记录，按时间倒序
    /// 是否将纯数字的 AutomationId 视为随机值（不参与共同特征匹配）
    pub ignore_numeric_automation_ids: bool,
    /// 是否在启动时启用 Narrator RunningState（通过注册表设置）
    pub enable_narrator_running_state: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            ignore_numeric_automation_ids: true,
            enable_narrator_running_state: true,
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
            process_name: "Weixin".to_string(),
        };
        
        let info2 = WindowInfo {
            title: "微信".to_string(),
            class_name: "mmui::MainWindow".to_string(),
            process_id: 12345,
            process_name: "Weixin".to_string(),
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
                process_name: "notepad".to_string(),
            }),
            capture_mode: CaptureMode::Fast,
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
            capture_mode: CaptureMode::Fast,
        };
        
        assert!(result.window_info.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 相似元素相关数据结构
// ═══════════════════════════════════════════════════════════════════════════════

/// 子元素特征（用于相似度比较）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildFeature {
    pub control_type: String,
    pub relative_bounds: RelativeRect,  // 相对于父元素的归一化坐标
}

/// 归一化的矩形坐标（相对于父元素）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelativeRect {
    pub x_ratio: f32,      // x / parent_width
    pub y_ratio: f32,      // y / parent_height
    pub width_ratio: f32,  // width / parent_width
    pub height_ratio: f32, // height / parent_height
}

impl RelativeRect {
    /// 从绝对坐标创建归一化坐标
    pub fn from_absolute(child_rect: &ElementRect, parent_rect: &ElementRect) -> Self {
        let parent_width = parent_rect.width.max(1) as f32;
        let parent_height = parent_rect.height.max(1) as f32;
        
        Self {
            x_ratio: child_rect.x as f32 / parent_width,
            y_ratio: child_rect.y as f32 / parent_height,
            width_ratio: child_rect.width as f32 / parent_width,
            height_ratio: child_rect.height as f32 / parent_height,
        }
    }
}

/// 相似元素样本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarElementSample {
    pub hierarchy_node: HierarchyNode,
    pub ancestor_chain: Vec<HierarchyNode>,  // 从根到父节点的链
    pub children_structure: Vec<ChildFeature>, // 子元素特征
}

/// 多元素共同祖先路径
#[derive(Debug, Clone)]
pub struct CommonAncestorPath {
    /// 共同祖先链（排除窗口节点，从第一个共同子节点到目标父级）
    pub common_ancestors: Vec<HierarchyNode>,
    /// 目标元素类型（所有样本一致才非空）
    pub target_control_type: String,
    /// 生成的搜索 XPath（如 //Group/ToolBar//Button）
    pub search_xpath: String,
}

/// 优化步骤（用户友好的进度显示）
#[derive(Debug, Clone)]
pub struct OptimizationStep {
    pub description: String,
    pub status: OptimizationStepStatus,
}

#[derive(Debug, Clone)]
pub enum OptimizationStepStatus {
    Done,
    InProgress,
    Skipped,
    Failed(String),
}

#[cfg(test)]
mod similarity_tests {
    use super::*;
    
    #[test]
    fn test_relative_rect_normalization() {
        let parent = ElementRect { x: 0, y: 0, width: 200, height: 100 };
        let child = ElementRect { x: 50, y: 25, width: 100, height: 50 };
        
        let rel = RelativeRect::from_absolute(&child, &parent);
        
        assert!((rel.x_ratio - 0.25).abs() < 0.01);   // 50/200 = 0.25
        assert!((rel.y_ratio - 0.25).abs() < 0.01);   // 25/100 = 0.25
        assert!((rel.width_ratio - 0.5).abs() < 0.01); // 100/200 = 0.5
        assert!((rel.height_ratio - 0.5).abs() < 0.01);// 50/100 = 0.5
    }
    
    #[test]
    fn test_relative_rect_zero_parent() {
        // 测试父元素尺寸为 0 的情况（应避免除零错误）
        let parent = ElementRect { x: 0, y: 0, width: 0, height: 0 };
        let child = ElementRect { x: 10, y: 10, width: 50, height: 50 };
        
        let rel = RelativeRect::from_absolute(&child, &parent);
        
        // 应该使用 max(1) 避免除零
        assert!(rel.x_ratio.is_finite());
        assert!(rel.y_ratio.is_finite());
        assert!(rel.width_ratio.is_finite());
        assert!(rel.height_ratio.is_finite());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 从 api/types.rs 下沉的类型（解决 core→api 依赖）
// ═══════════════════════════════════════════════════════════════════════════════

/// 2D 点坐标
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 矩形区域（API 层使用，与 ElementRect 不同）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// 计算中心点
    pub fn center(&self) -> Point {
        Point::new(
            self.x + self.width / 2,
            self.y + self.height / 2,
        )
    }
}

/// 各方向超出视口的像素数
#[derive(Debug, Clone, Serialize)]
pub struct OverflowInfo {
    /// 元素顶部超出视口顶部的像素（正值=元素在视口上方）
    pub top: i32,
    /// 元素底部超出视口底部的像素（正值=元素在视口下方）
    pub bottom: i32,
    /// 元素左侧超出视口左侧的像素（正值=元素在视口左边）
    pub left: i32,
    /// 元素右侧超出视口右侧的像素（正值=元素在视口右边）
    pub right: i32,
}

/// 元素可视性计算结果（core 层纯数据，无 serde）
///
/// 由 `compute_visibility` 返回，API 层负责转换为 `ElementVisibilityResponse`。
/// 不依赖 api 层类型，解决 core→api 反向依赖。
#[derive(Debug, Clone)]
pub struct VisibilityResult {
    pub found: bool,
    pub is_offscreen: Option<bool>,
    pub visibility: String,
    pub position: String,
    pub element_rect: Option<Rect>,
    pub visible_rect: Option<Rect>,
    pub viewport_rect: Option<Rect>,
    pub overflow: Option<OverflowInfo>,
    pub scroll_direction: Option<String>,
    pub error: Option<String>,
}

/// 元素数据（core 层，无 serde）
///
/// 由 `element_info_from_uia` 等函数返回，API 层负责转换为 `api::types::ElementInfo`。
/// 与 `ElementInfo` 字段一一对应，但不含 serde 注解，解决 core→api 反向依赖。
#[derive(Debug, Clone)]
pub struct ElementData {
    pub rect: Option<Rect>,
    pub visible_rect: Option<Rect>,
    pub center: Option<Point>,
    pub center_random: Option<Point>,
    pub control_type: String,
    pub name: String,
    pub automation_id: String,
    pub class_name: String,
    pub framework_id: String,
    pub help_text: String,
    pub localized_control_type: String,
    pub is_enabled: bool,
    pub is_offscreen: bool,
    pub is_password: bool,
    pub accelerator_key: String,
    pub access_key: String,
    pub item_type: String,
    pub item_status: String,
    pub process_id: u32,
    pub runtime_id: Option<String>,
    pub is_checkable: Option<bool>,
    pub is_checked: Option<bool>,
    pub is_clickable: Option<bool>,
    pub is_scrollable: Option<bool>,
    pub is_selected: Option<bool>,
}

/// 导航步骤类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NavigateStep {
    /// 向上 N 层父元素
    #[serde(rename = "parent")]
    Parent { levels: u32 },
    /// 第 N 个子元素（0-based，负数表示倒数）
    #[serde(rename = "child")]
    Child { index: i32 },
    /// 绝对位置兄弟元素（0-based，负数表示倒数）
    #[serde(rename = "sibling_abs")]
    SiblingAbs { index: i32 },
    /// 左侧第 N 个兄弟（相对偏移）
    #[serde(rename = "sibling_left")]
    SiblingLeft { offset: u32 },
    /// 右侧第 N 个兄弟（相对偏移）
    #[serde(rename = "sibling_right")]
    SiblingRight { offset: u32 },
}