// src/core/metrics.rs
//
// 共享的性能指标工具：请求 ID 分配、选择器哈希、XPath 元信息
// API 层和 COM 层统一使用此模块，实现端到端链路追踪

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// 全局请求 ID 序列（API 层和 COM 层共享，实现端到端追踪）
static REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

/// 分配下一个请求 ID
pub fn next_request_id() -> u64 {
    REQUEST_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 对选择器字符串计算稳定哈希，用于日志脱敏
pub fn selector_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// 生成 XPath 元信息摘要字符串，用于性能日志
pub fn xpath_meta(xpath: &str) -> String {
    format!(
        "xpath_hash={:016x} xpath_len={} descendant={}",
        selector_hash(xpath),
        xpath.len(),
        xpath.starts_with("//") || xpath.contains("//")
    )
}
