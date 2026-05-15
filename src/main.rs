// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// GUI 模块
mod gui;

use gui::SelectorApp;
use gui::mouse_hook;

use eframe::egui;
use log::info;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();
    info!("element-selector starting");

    // COM must be initialized on the main thread (STA) for UI Automation.
    {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("CoInitializeEx failed");
        }
    }

    // Initialize the global mouse hook system.
    mouse_hook::init()
        .expect("Failed to initialize mouse hook system");
    info!("Mouse hook system initialized");

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
