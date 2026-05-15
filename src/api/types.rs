// src/api/types.rs
//
// API 请求/响应类型定义

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// 通用结构
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

/// 矩形区域
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

// ═══════════════════════════════════════════════════════════════════════════════
// 窗口 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 窗口信息（API 响应格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfoResponse {
    pub title: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
    #[serde(rename = "processName")]
    pub process_name: String,
}

/// 窗口列表响应
#[derive(Debug, Clone, Serialize)]
pub struct WindowListResponse {
    pub windows: Vec<WindowInfoResponse>,
}

/// 窗口选择器条件
#[derive(Debug, Clone, Deserialize)]
pub struct WindowSelector {
    pub title: Option<String>,
    #[serde(rename = "className")]
    pub class_name: Option<String>,
    #[serde(rename = "processName")]
    pub process_name: Option<String>,
}

/// 窗口选择器（支持字符串或对象形式）
#[derive(Debug, Clone)]
pub enum WindowSelectorOrString {
    /// 字符串形式："Window[@Name='xxx' and @ClassName='yyy']"
    String(String),
    /// 对象形式：{title: "xxx", className: "yyy"}
    Object(WindowSelector),
}

impl<'de> serde::Deserialize<'de> for WindowSelectorOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        
        struct WindowSelectorVisitor;
        
        impl<'de> Visitor<'de> for WindowSelectorVisitor {
            type Value = WindowSelectorOrString;
            
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a WindowSelector object")
            }
            
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WindowSelectorOrString::String(value.to_string()))
            }
            
            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WindowSelectorOrString::String(value))
            }
            
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let selector = WindowSelector::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(WindowSelectorOrString::Object(selector))
            }
        }
        
        deserializer.deserialize_any(WindowSelectorVisitor)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素查询参数（GET 请求）
#[derive(Debug, Clone, Deserialize)]
pub struct ElementQuery {
    /// 窗口选择器，如 "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    #[serde(rename = "windowSelector")]
    pub window_selector: String,
    /// 元素 XPath，如 "//Button[@AutomationId='btnSend']"
    pub xpath: String,
    /// 随机坐标范围百分比（默认 0.55）
    #[serde(rename = "randomRange", default = "default_random_range")]
    pub random_range: f32,
}

fn default_random_range() -> f32 {
    0.55
}

/// 元素信息（API 响应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    pub rect: Rect,
    pub center: Point,
    #[serde(rename = "centerRandom")]
    pub center_random: Point,
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
    #[serde(rename = "automationId")]
    pub automation_id: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    #[serde(rename = "helpText")]
    pub help_text: String,
    #[serde(rename = "localizedControlType")]
    pub localized_control_type: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    #[serde(rename = "isPassword")]
    pub is_password: bool,
    #[serde(rename = "acceleratorKey")]
    pub accelerator_key: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "itemStatus")]
    pub item_status: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
}

/// 元素查找响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementResponse {
    pub found: bool,
    pub element: Option<ElementInfo>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 鼠标 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 鼠标移动选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseMoveOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 轨迹类型: "linear" | "bezier"
    #[serde(default = "default_trajectory")]
    pub trajectory: String,
    /// 移动持续时间（毫秒）
    #[serde(default = "default_duration")]
    pub duration: u64,
}

fn default_humanize() -> bool { true }
fn default_trajectory() -> String { "bezier".to_string() }
fn default_duration() -> u64 { 600 }

/// 鼠标移动请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseMoveRequest {
    pub target: Point,
    pub options: Option<MouseMoveOptions>,
}

/// 鼠标移动响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseMoveResponse {
    pub success: bool,
    #[serde(rename = "startPoint")]
    pub start_point: Point,
    #[serde(rename = "endPoint")]
    pub end_point: Point,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// 鼠标点击选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseClickOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 随机坐标范围百分比
    #[serde(rename = "randomRange", default = "default_random_range")]
    pub random_range: f32,
    /// 点击前停顿（毫秒）
    #[serde(rename = "pauseBefore", default)]
    pub pause_before: u64,
    /// 点击后停顿（毫秒）
    #[serde(rename = "pauseAfter", default)]
    pub pause_after: u64,
}

/// 鼠标点击请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseClickRequest {
    /// 窗口选择器，支持字符串形式 "Window[@Name='xxx']" 或对象形式
    pub window: WindowSelectorOrString,
    pub xpath: String,
    pub options: Option<MouseClickOptions>,
}

/// 点击的元素信息摘要
#[derive(Debug, Clone, Serialize)]
pub struct ClickedElement {
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
}

/// 鼠标点击响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseClickResponse {
    pub success: bool,
    #[serde(rename = "clickPoint")]
    pub click_point: Point,
    pub element: Option<ClickedElement>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 辅助函数
// ═══════════════════════════════════════════════════════════════════════════════

impl From<super::super::model::ElementRect> for Rect {
    fn from(r: super::super::model::ElementRect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl From<super::super::model::WindowInfo> for WindowInfoResponse {
    fn from(w: super::super::model::WindowInfo) -> Self {
        Self {
            title: w.title,
            class_name: w.class_name,
            process_id: w.process_id,
            process_name: w.process_name,
        }
    }
}