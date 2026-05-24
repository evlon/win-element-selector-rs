// src/gui/iced_style.rs
//
// iced 主题与样式（从旧 Theme 迁移颜色）

use iced::Color;

/// 主题颜色（从旧 egui::Color32 映射为 iced::Color）
#[derive(Clone, Copy)]
pub struct ThemeColors {
    pub sel_bg:          Color,
    pub sel_fg:          Color,
    pub panel_hdr:       Color,
    pub border:          Color,
    pub target_fg:       Color,
    pub ok:              Color,
    pub err:             Color,
    pub warn:            Color,
    pub muted:           Color,
    pub mono_fg:         Color,
    pub panel_fill:      Color,
    pub central_bg:      Color,
    pub top_bar_bg:      Color,
    pub bottom_bg:       Color,
    pub preview_bg:      Color,
    pub info_bg:         Color,
    pub segment_bg:      Color,
    pub text:            Color,
    pub hdr_text:        Color,
    pub val_found_bg:    Color,
    pub val_notfound_bg: Color,
    pub fail_step_bg:    Color,
    pub warn_detail_fg:  Color,
}

/// RGB 辅助函数
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

impl ThemeColors {
    /// 浅色主题
    pub fn light() -> Self {
        Self {
            sel_bg:          rgb(219, 234, 254),
            sel_fg:          rgb(30,  64,  175),
            panel_hdr:       rgb(241, 245, 249),
            border:          rgb(203, 213, 225),
            target_fg:       rgb(30,  64,  175),
            ok:              rgb(22,  163, 74),
            err:             rgb(220, 38,  38),
            warn:            rgb(202, 138, 4),
            muted:           rgb(107, 114, 128),
            mono_fg:         rgb(37,  99,  235),
            panel_fill:      Color::WHITE,
            central_bg:      Color::from_rgb8(250, 250, 250),
            top_bar_bg:      Color::from_rgb8(248, 248, 248),
            bottom_bg:       Color::from_rgb8(245, 245, 245),
            preview_bg:      Color::from_rgb8(252, 252, 252),
            info_bg:         rgb(239, 246, 255),
            segment_bg:      rgb(248, 250, 252),
            text:            Color::from_rgb8(35,  35,  35),
            hdr_text:        Color::from_rgb8(70,  70,  70),
            val_found_bg:    rgb(245, 255, 245),
            val_notfound_bg: rgb(255, 245, 245),
            fail_step_bg:    rgb(255, 250, 240),
            warn_detail_fg:  rgb(200, 100, 0),
        }
    }
}