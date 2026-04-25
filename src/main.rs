// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod error;
mod highlight;
mod model;
mod xpath;

use eframe::egui;
use log::info;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();
    info!("element-selector starting");

    // COM must be initialized on the main thread (STA) for UI Automation.
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("CoInitializeEx failed");
        }
    }

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
            
            // Try to load system Chinese font
            #[cfg(target_os = "windows")]
            {
                // Use Microsoft YaHei (微软雅黑) which is built-in on Windows
                let font_data = egui::FontData::from_static(include_bytes!(
                    "C:/Windows/Fonts/msyh.ttc"
                ));
                fonts.font_data.insert("msyh".to_owned(), font_data);
                
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
            Ok(Box::new(app::SelectorApp::new(cc)) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}

fn load_icon() -> egui::IconData {
    // 16x16 RGBA placeholder icon (blue square)
    let size = 16usize;
    let mut pixels = vec![0u8; size * size * 4];
    for chunk in pixels.chunks_mut(4) {
        chunk[0] = 44;   // R
        chunk[1] = 82;   // G
        chunk[2] = 130;  // B
        chunk[3] = 255;  // A
    }
    egui::IconData { rgba: pixels, width: size as u32, height: size as u32 }
}
