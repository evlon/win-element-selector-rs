// src/gui/logger.rs
//
// GUI 内嵌日志面板 - 线程安全的日志收集器
// 同时输出到日志文件（每次启动覆盖）

use std::sync::{Arc, Mutex};
use log::{Level, LevelFilter, Log, Metadata, Record};

/// 日志文件路径（相对于 exe 所在目录的 log/debug/element-selector.log）
const LOG_FILE_RELATIVE: &str = "log/debug/element-selector.log";

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

/// 获取当前本地时间字符串（精确到毫秒），格式: HH:MM:SS.sss
fn local_time_ms() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    unsafe {
        let st = GetLocalTime();
        format!("{:02}:{:02}:{:02}.{:03}",
            st.wHour, st.wMinute, st.wSecond, st.wMilliseconds)
    }
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
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
    }

    /// 判断是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.logs.lock().unwrap().is_empty()
    }
}

/// 组合日志器：同时输出到 GUI 面板、标准输出和日志文件
/// 作为唯一的全局 logger，替代 env_logger + GuiLogger 双注册冲突
struct CombinedLogger {
    gui: GuiLogger,
    file: Arc<Mutex<std::fs::File>>,
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
            let timestamp = local_time_ms();
            let line = format!("{} [{}] {}", timestamp, level_str, msg);
            // 输出到控制台
            println!("{}", line);
            // 写入日志文件
            if let Ok(mut file) = self.file.lock() {
                use std::io::Write;
                let _ = writeln!(file, "{}", line);
            }
            // 添加到 GUI 日志面板
            self.gui.add_log(record.level(), msg);
        }
    }

    fn flush(&self) {
        if let Ok(file) = self.file.lock() {
            let _ = file.sync_all();
        }
    }
}

/// 初始化全局组合日志器（GUI 面板 + 控制台 + 日志文件）
/// 每次启动覆盖日志文件
pub fn init_gui_logger(max_lines: usize) {
    let log_path = get_log_file_path();
    // 确保目录存在
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 创建/覆盖日志文件，写入 UTF-8 BOM 以便 Windows 记事本等工具正确识别编码
    let mut file = std::fs::File::create(&log_path)
        .unwrap_or_else(|e| {
            eprintln!("无法创建日志文件 {:?}: {}", log_path, e);
            std::fs::File::create(std::env::temp_dir().join("element-selector.log"))
                .expect("连临时目录都无法创建日志文件")
        });
    // 写入 UTF-8 BOM (EF BB BF)，让编辑器/记事本自动识别为 UTF-8
    {
        use std::io::Write;
        let _ = file.write_all(&[0xEF, 0xBB, 0xBF]);
    }

    let logger = CombinedLogger {
        gui: GuiLogger::new(max_lines),
        file: Arc::new(Mutex::new(file)),
    };
    let gui = logger.gui.clone();
    log::set_boxed_logger(Box::new(logger)).expect("failed to set logger");
    log::set_max_level(LevelFilter::Info);
    GUI_LOGGER.with(|s| *s.borrow_mut() = Some(gui));
    
    log::info!("日志文件: {:?}", log_path);
}

/// 获取日志文件路径：exe 所在目录 / log/debug/element-selector.log
fn get_log_file_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join(LOG_FILE_RELATIVE);
        }
    }
    // 回退到当前工作目录
    std::path::PathBuf::from(LOG_FILE_RELATIVE)
}

thread_local! {
    static GUI_LOGGER: std::cell::RefCell<Option<GuiLogger>> = const { std::cell::RefCell::new(None) };
}

/// 从 UI 线程获取 GUI Logger 引用（用于清空等操作）
pub fn get_gui_logger() -> Option<GuiLogger> {
    GUI_LOGGER.with(|s| s.borrow().clone())
}
