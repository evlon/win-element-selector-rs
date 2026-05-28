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
            
            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
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
    pub window: String,
    /// 元素 XPath，如 "//Button[@AutomationId='btnSend']"
    pub element: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    #[serde(rename = "centerRandom", skip_serializing_if = "Option::is_none")]
    pub center_random: Option<Point>,
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
    // ─── UIA Pattern availability ──────────────────────────────────────────
    #[serde(default, rename = "isCheckable", skip_serializing_if = "Option::is_none")]
    pub is_checkable: Option<bool>,
    #[serde(default, rename = "isChecked", skip_serializing_if = "Option::is_none")]
    pub is_checked: Option<bool>,
    #[serde(default, rename = "isClickable", skip_serializing_if = "Option::is_none")]
    pub is_clickable: Option<bool>,
    #[serde(default, rename = "isScrollable", skip_serializing_if = "Option::is_none")]
    pub is_scrollable: Option<bool>,
    #[serde(default, rename = "isSelected", skip_serializing_if = "Option::is_none")]
    pub is_selected: Option<bool>,
}

/// 元素查找响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementResponse {
    pub found: bool,
    #[serde(rename = "findSelector")]
    pub element_selector: String,
    pub element: Option<ElementInfo>,
    /// 匹配到的元素总数（findOne 返回第一个但告知总共有多少个匹配）
    pub total: usize,
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
    /// 点击按钮类型: "left" | "right"
    #[serde(default = "default_button")]
    pub button: String,
    /// 点击区域限制（按比例缩小可点击范围）
    #[serde(rename = "clickArea", default)]
    pub click_area: Option<ClickArea>,
    /// 是否在点击位置留痕（红色圆点标记）
    #[serde(rename = "markClick", default)]
    pub mark_click: bool,
    /// 留痕超时时间（毫秒），默认 3000
    #[serde(rename = "markTimeout", default = "default_mark_timeout")]
    pub mark_timeout: u64,
}

/// 点击区域限制（按比例）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClickArea {
    /// 左侧排除比例（0-1），如 0.3 表示左侧 30% 区域不点击
    pub left: Option<f32>,
    /// 右侧排除比例（0-1），如 0.3 表示右侧 30% 区域不点击
    pub right: Option<f32>,
    /// 顶部排除比例（0-1）
    pub top: Option<f32>,
    /// 底部排除比例（0-1）
    pub bottom: Option<f32>,
}

fn default_button() -> String { "left".to_string() }
fn default_mark_timeout() -> u64 { 3000 }

/// 鼠标点击请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseClickRequest {
    /// 窗口选择器，支持字符串形式 "Window[@Name='xxx']" 或对象形式
    pub window: WindowSelectorOrString,
    pub element: String,
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
// 滚动 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 滚动选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseScrollOptions {
    /// WHEEL_DELTA 单位，默认 120
    pub delta: Option<i32>,
    /// 滚动次数，默认 30
    #[serde(default = "default_scroll_times")]
    pub times: Option<u32>,
    /// 等待出现的 xpath
    pub wait: Option<String>,
    /// 等待超时 ms，默认 5000
    pub timeout: Option<u64>,
    /// 是否自动计算 delta（基于容器高度），默认 false
    #[serde(rename = "autoDelta", default)]
    pub auto_delta: Option<bool>,
    /// 容器高度倍率（0-1），默认 0.8
    #[serde(rename = "deltaFactor", default = "default_delta_factor")]
    pub delta_factor: Option<f32>,
    /// 等待模式：exist=元素存在即可，visible=元素存在且不在屏幕外
    #[serde(rename = "waitMode", default)]
    pub wait_mode: Option<String>,
    /// 是否滚动到视口中心（仅 visible 模式生效），默认 true
    #[serde(rename = "scrollToCenter", default = "default_scroll_to_center")]
    pub scroll_to_center: Option<bool>,
    /// scrollToCenter 模式下，元素可见后继续调整到视口中心的最大滚动次数，避免死循环，默认 5
    #[serde(rename = "scrollToCenterAdjustTimes", default = "default_scroll_to_center_adjust_times")]
    pub scroll_to_center_adjust_times: Option<u32>,
}

fn default_scroll_to_center() -> Option<bool> { Some(true) }
fn default_scroll_to_center_adjust_times() -> Option<u32> { Some(5) }

fn default_scroll_times() -> Option<u32> { Some(30) }
fn default_delta_factor() -> Option<f32> { Some(0.8) }

/// 滚动请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseScrollRequest {
    /// 窗口选择器（用于限定搜索范围，避免遍历所有窗口）
    pub window: Option<String>,
    pub element: String,
    pub options: Option<MouseScrollOptions>,
}

/// 滚动响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseScrollResponse {
    pub success: bool,
    /// 实际滚动次数
    pub scrolled: u32,
    /// 是否找到目标
    #[serde(rename = "targetFound")]
    pub target_found: bool,
    /// 目标元素的矩形区域（仅当 target_found=true 时有值）
    #[serde(rename = "targetRect", skip_serializing_if = "Option::is_none")]
    pub target_rect: Option<Rect>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 悬停 & 拖拽 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 悬停选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseHoverOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 悬停停留时间（毫秒），默认 500
    #[serde(default = "default_hover_duration")]
    pub duration: u64,
}

fn default_hover_duration() -> u64 { 500 }

/// 悬停请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseHoverRequest {
    /// 窗口选择器
    pub window: WindowSelectorOrString,
    /// 元素 XPath
    pub element: String,
    pub options: Option<MouseHoverOptions>,
}

/// 悬停响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseHoverResponse {
    pub success: bool,
    pub hover_point: Point,
    pub error: Option<String>,
}

/// 拖拽选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseDragOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 拖拽持续时间（毫秒）
    #[serde(default = "default_duration")]
    pub duration: u64,
}

/// 拖拽请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseDragRequest {
    /// 源元素窗口选择器
    pub window: WindowSelectorOrString,
    /// 源元素 XPath
    #[serde(rename = "sourceElement")]
    pub source_element: String,
    /// 目标元素 XPath
    #[serde(rename = "targetElement")]
    pub target_element: String,
    pub options: Option<MouseDragOptions>,
}

/// 拖拽响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseDragResponse {
    pub success: bool,
    pub source_point: Point,
    pub target_point: Point,
    pub duration_ms: u64,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素可视区域位置 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素可视区域位置查询请求
#[derive(Debug, Clone, Deserialize)]
pub struct ElementVisibilityRequest {
    /// 窗口选择器
    pub window: String,
    /// 元素 XPath
    pub element: String,
}

/// 元素可视区域位置响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementVisibilityResponse {
    /// 是否找到元素
    pub found: bool,
    /// UIA 的 IsOffscreen 属性
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: Option<bool>,
    /// 可视性：fully_visible / partially_visible / offscreen
    pub visibility: String,
    /// 相对位置：above / below / left / right / inside / partial
    pub position: String,
    /// 元素的边界矩形
    #[serde(rename = "elementRect", skip_serializing_if = "Option::is_none")]
    pub element_rect: Option<Rect>,
    /// 窗口（视口）的边界矩形
    #[serde(rename = "viewportRect", skip_serializing_if = "Option::is_none")]
    pub viewport_rect: Option<Rect>,
    /// 元素在各方向超出视口的像素数（正值=超出，0=在视口内）
    #[serde(rename = "overflow", skip_serializing_if = "Option::is_none")]
    pub overflow: Option<OverflowInfo>,
    /// 建议滚动方向：up / down / left / right / null
    #[serde(rename = "scrollDirection", skip_serializing_if = "Option::is_none")]
    pub scroll_direction: Option<String>,
    pub error: Option<String>,
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

// ═══════════════════════════════════════════════════════════════════════════════
// 元素高亮闪烁 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素高亮闪烁请求
#[derive(Debug, Clone, Deserialize)]
pub struct ElementFlashRequest {
    /// 窗口选择器
    pub window: String,
    /// 元素 XPath
    pub element: String,
    /// 闪烁持续时间（毫秒），默认 1000
    #[serde(default = "default_flash_timeout")]
    pub timeout: u64,
}

fn default_flash_timeout() -> u64 { 1000 }

/// 元素高亮闪烁响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementFlashResponse {
    pub success: bool,
    #[serde(rename = "elementRect", skip_serializing_if = "Option::is_none")]
    pub element_rect: Option<Rect>,
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