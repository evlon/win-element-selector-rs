// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// GUI 模块
mod gui;

use gui::SelectorApp;
use gui::input_hook;

use eframe::egui;
use log::info;

fn main() -> anyhow::Result<()> {
    // 【禁用 egui 警告】防止 Widget rect changed id 警告干扰调试
    std::env::set_var("EGUI_WARN_ID_CHANGES", "0");
    
    // 【Windows 控制台编码修复】设置控制台为 UTF-8
    #[cfg(windows)]
    {
        use windows::Win32::System::Console::SetConsoleOutputCP;
        unsafe {
            // CP_UTF8 = 65001
            let _ = SetConsoleOutputCP(65001);
        }
    }
    
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    // Show help if requested
    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        println!("Windows Element Selector v1.0.1");
        println!();
        println!("USAGE:");
        println!("    element-selector [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("    -v          Set log level to WARN");
        println!("    -vv         Set log level to INFO (default)");
        println!("    -vvv        Set log level to DEBUG (verbose mode)");
        println!("    --verbose   Same as -vvv");
        println!("    -h, --help  Show this help message");
        println!();
        println!("EXAMPLES:");
        println!("    element-selector              # Normal mode (INFO level)");
        println!("    element-selector -vvv         # Verbose mode (DEBUG level)");
        println!("    element-selector --verbose    # Verbose mode (DEBUG level)");
        return Ok(());
    }
    
    // 【关键修复】不再使用 env_logger，改用 GuiLogger
    // GuiLogger 会在 App::new() 中初始化，同时输出到控制台和 GUI

    // COM must be initialized on the main thread (STA) for UI Automation.
    {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("CoInitializeEx failed");
        }
    }

    // Initialize the global input hook system (rdev grab).
    input_hook::init()
        .expect("Failed to initialize input hook system");
    info!("Input hook system initialized");

    // Initialize global COM worker thread (single-threaded COM management)
    element_selector::core::com_worker::init_global_com_worker()
        .expect("Failed to initialize COM worker");
    info!("COM worker thread initialized");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Windows 元素选择器 v1.0")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([760.0, 500.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Windows 元素选择器",
        options,
        Box::new(|cc| {
            // Configure fonts for Chinese character support
            let mut fonts = egui::FontDefinitions::default();
            
            // Load system Chinese font
            {
                // Use Microsoft YaHei (微软雅黑) which is built-in on Windows
                let font_data = egui::FontData::from_static(include_bytes!(
                    "C:/Windows/Fonts/msyh.ttc"
                ));
                fonts.font_data.insert("msyh".to_owned(), std::sync::Arc::new(font_data));
                
                // Insert at the beginning of font families
                for family in [
                    egui::FontFamily::Proportional,
                    egui::FontFamily::Monospace,
                ] {
                    fonts.families.get_mut(&family).unwrap()
                        .insert(0, "msyh".to_owned());
                }
            }
            
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(SelectorApp::new(cc)) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}

fn load_icon() -> egui::IconData {
    let icon_rgba = include_bytes!("../assets/icon_32.rgba");
    let size = 32u32;
    egui::IconData {
        rgba: icon_rgba.to_vec(),
        width: size,
        height: size,
    }
}
