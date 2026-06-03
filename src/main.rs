// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// GUI 模块
mod gui;

use gui::iced_app::{State, update, view, subscription};
use gui::input_hook;
use gui::persistence::load_config;

use log::info;

fn main() -> anyhow::Result<()> {
    // 【Windows 控制台编码修复】设置控制台为 UTF-8
    #[cfg(windows)]
    {
        use windows::Win32::System::Console::SetConsoleOutputCP;
        unsafe {
            // CP_UTF8 = 65001
            let _ = SetConsoleOutputCP(65001);
        }
    }

    gui::logger::init_gui_logger(1000);

    // Parse command line args for narrator override
    let args: Vec<String> = std::env::args().collect();
    let cli_skip_narrator = args.contains(&"--no-narrator-motion".to_string());

    // COM must be initialized in MTA mode for UI Automation (free-threaded).
    // All threads using UIA will share the global IUIAutomation instance.
    element_selector::core::uia_context::init_uia_context()
        .expect("Failed to initialize UIA context (MTA)");
    info!("UIA context initialized (MTA mode)");

    // Enable Narrator RunningState for full UIA tree visibility (configurable).
    let config = load_config();
    let _narrator_guard = if !cli_skip_narrator
        && config.enable_narrator_running_state
        && element_selector::core::narrator::should_enable()
    {
        Some(element_selector::core::narrator::enable_narrator_running_state())
    } else {
        info!("Narrator RunningState: skipped (--no-narrator-motion or config/env override)");
        None
    };

    // Initialize the global input hook system (rdev grab).
    input_hook::init()
        .expect("Failed to initialize input hook system");
    info!("Input hook system initialized");

    // Load fonts from Windows system fonts
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = {
        let font_paths = [
            "C:\\Windows\\Fonts\\msyh.ttc",       // 微软雅黑 - 中文
            "C:\\Windows\\Fonts\\msyhbd.ttc",     // 微软雅黑 Bold
            "C:\\Windows\\Fonts\\msyhl.ttc",      // 微软雅黑 Light
            "C:\\Windows\\Fonts\\seguisym.ttf",   // Segoe UI Symbol - ✓✗等符号
            "C:\\Windows\\Fonts\\segoeui.ttf",    // Segoe UI - 西文
        ];
        let mut loaded = Vec::new();
        for path in &font_paths {
            if let Ok(data) = std::fs::read(path) {
                info!("Loaded font: {}", path);
                loaded.push(std::borrow::Cow::Owned(data));
            }
        }
        if loaded.is_empty() {
            log::warn!("No fonts loaded from Windows\\Fonts");
        }
        loaded
    };

    let default_font = iced::Font::with_name("Microsoft YaHei");

    // Launch iced GUI
    let mut app = iced::application(State::init, update, view)
        .title("Windows 元素选择器 v1.0")
        .subscription(subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(980.0, 660.0),
            min_size: Some(iced::Size::new(760.0, 500.0)),
            ..Default::default()
        })
        .default_font(default_font);

    for font_data in fonts {
        app = app.font(font_data);
    }

    app.run()
        .map_err(|e| anyhow::anyhow!("iced error: {}", e))
}
