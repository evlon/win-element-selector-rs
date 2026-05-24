// src/gui/persistence.rs
//
// 文件持久化配置（替代 eframe::Storage）

use std::path::PathBuf;
use log::{info, warn};

use element_selector::core::model::AppConfig;

/// 配置持久化目录
fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("element-selector")
}

/// 配置文件路径
fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// 最后一次捕获文件路径
fn capture_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("last_capture.json")
}

/// 加载 AppConfig
pub fn load_config() -> AppConfig {
    let path = config_path();
    if !path.exists() {
        return AppConfig::default();
    }
    match std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
    {
        Some(config) => {
            info!("Loaded config from {}", path.display());
            config
        }
        None => {
            warn!("Failed to parse config.json, using defaults");
            AppConfig::default()
        }
    }
}

/// 保存 AppConfig
pub fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        if let Err(e) = std::fs::write(&path, json) {
            warn!("Failed to save config: {}", e);
        } else {
            info!("Saved config to {}", path.display());
        }
    }
}

/// 保存最后一次捕获
pub fn save_capture(json: &str) {
    let path = capture_path();
    if let Err(e) = std::fs::write(&path, json) {
        warn!("Failed to save capture: {}", e);
    } else {
        info!("Saved capture to {}", path.display());
    }
}

/// 加载最后一次捕获的 JSON 原始字符串
pub fn load_capture_json() -> Option<String> {
    let path = capture_path();
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path).ok()
}
