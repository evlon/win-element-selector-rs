// src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SelectorError {
    #[error("UI Automation 初始化失败: {0}")]
    ComInit(String),

    #[error("无法创建 IUIAutomation 实例: {0}")]
    AutomationCreate(String),

    #[error("ElementFromPoint 失败 ({x},{y}): {source}")]
    ElementFromPoint {
        x: i32,
        y: i32,
        #[source]
        source: anyhow::Error,
    },

    #[error("获取属性失败 [{property}]: {source}")]
    GetProperty {
        property: &'static str,
        #[source]
        source: anyhow::Error,
    },

    #[error("遍历父节点失败: {0}")]
    TreeWalk(String),

    #[error("高亮窗口创建失败: {0}")]
    Highlight(String),

    #[error("XPath 校验失败: {0}")]
    Validation(String),

    #[error("剪贴板操作失败: {0}")]
    Clipboard(String),
}

pub type Result<T> = std::result::Result<T, SelectorError>;
