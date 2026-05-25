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

    /// 判断是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.logs.lock().unwrap().is_empty()
    }
}

/// 组合日志器：同时输出到 GUI 面板和标准输出
/// 作为唯一的全局 logger，替代 env_logger + GuiLogger 双注册冲突
struct CombinedLogger {
    gui: GuiLogger,
}

impl Log for CombinedLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            let level_str = match record.level() {
                Level::Error => "ERROR",
                Level::Warn  => "WARN ",
                Level::Info  => "INFO ",
                Level::Debug => "DEBUG",
                Level::Trace => "TRACE",
            };
            // 输出到控制台
            println!("[{}] {}", level_str, msg);
            // 同时添加到 GUI 日志面板
            self.gui.add_log(record.level(), msg);
        }
    }

    fn flush(&self) {}
}

/// 初始化全局组合日志器（GUI 面板 + 控制台）
pub fn init_gui_logger(max_lines: usize) {
    let logger = CombinedLogger {
        gui: GuiLogger::new(max_lines),
    };
    let gui = logger.gui.clone();
    log::set_boxed_logger(Box::new(logger)).expect("failed to set logger");
    log::set_max_level(LevelFilter::Info);
    GUI_LOGGER.with(|s| *s.borrow_mut() = Some(gui));
}

thread_local! {
    static GUI_LOGGER: std::cell::RefCell<Option<GuiLogger>> = const { std::cell::RefCell::new(None) };
}

/// 从 UI 线程获取 GUI Logger 引用（用于清空等操作）
pub fn get_gui_logger() -> Option<GuiLogger> {
    GUI_LOGGER.with(|s| s.borrow().clone())
}
