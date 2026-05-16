// src/gui/logger.rs
//
// GUI 内嵌日志面板 - 线程安全的日志收集器

use std::sync::{Arc, Mutex};
use log::{Level, LevelFilter, Log, Metadata, Record};

/// 线程安全的 GUI 日志收集器
#[derive(Clone)]
pub struct GuiLogger {
    logs: Arc<Mutex<Vec<LogEntry>>>,
    max_lines: usize,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: Level,
    pub message: String,
    pub timestamp: std::time::SystemTime,
}

impl GuiLogger {
    pub fn new(max_lines: usize) -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
            max_lines,
        }
    }
    
    /// 添加日志条目
    pub fn add_log(&self, level: Level, message: String) {
        let mut logs = self.logs.lock().unwrap();
        logs.push(LogEntry {
            level,
            message,
            timestamp: std::time::SystemTime::now(),
        });
        
        // 限制日志行数，避免内存溢出
        if logs.len() > self.max_lines {
            let remove_count = logs.len() - self.max_lines;
            logs.drain(0..remove_count);
        }
    }
    
    /// 获取所有日志（按时间顺序）
    pub fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.lock().unwrap().clone()
    }
    
    /// 清空日志
    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
    }
    
    /// 获取日志数量
    pub fn len(&self) -> usize {
        self.logs.lock().unwrap().len()
    }
    
    /// 判断是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.logs.lock().unwrap().is_empty()
    }
}

// 实现 log::Log trait
impl Log for GuiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info  // 只收集 INFO 及以上级别
    }
    
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            
            // 【关键修复】同时输出到标准输出和控制台
            // 这样后台线程的日志也能被看到
            eprintln!("[{}] {}", record.level(), msg);
            
            // 同时添加到 GUI 日志面板
            self.add_log(record.level(), msg);
        }
    }
    
    fn flush(&self) {}
}

/// 初始化 GUI Logger
pub fn init_gui_logger(max_lines: usize) -> GuiLogger {
    let logger = GuiLogger::new(max_lines);
    let _ = log::set_boxed_logger(Box::new(logger.clone()));
    log::set_max_level(LevelFilter::Info);
    logger
}
