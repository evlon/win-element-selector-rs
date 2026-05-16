// src/gui/theme.rs
//
// UI 主题配置和颜色定义

use eframe::egui::Color32;

#[derive(Clone, Copy)]
pub struct Theme {
    pub title_bg:        Color32,
    pub title_fg:        Color32,
    pub sel_bg:          Color32,
    pub sel_fg:          Color32,
    pub panel_hdr:       Color32,
    pub border:          Color32,
    pub target_fg:       Color32,
    pub ok:              Color32,
    pub err:             Color32,
    pub warn:            Color32,
    pub muted:           Color32,
    pub mono_fg:         Color32,
    pub divider:         Color32,
    pub capture_bg:      Color32,
    pub capture_fg:      Color32,
    pub panel_fill:      Color32,
    pub central_bg:      Color32,
    pub top_bar_bg:      Color32,
    pub bottom_bg:       Color32,
    pub preview_bg:      Color32,
    pub info_bg:         Color32,
    pub row_even:        Color32,
    pub row_odd:         Color32,
    pub segment_bg:      Color32,
    pub text:            Color32,
    pub btn_text:        Color32,
    pub hdr_text:        Color32,
    pub val_found_bg:    Color32,
    pub val_notfound_bg: Color32,
    pub fail_step_bg:    Color32,
    pub expected_fg:     Color32,
    pub actual_fg:       Color32,
    pub warn_detail_fg:  Color32,
    pub confirm_off:     Color32,
    pub history_fg:      Color32,
    pub capture_border:  Color32,
    pub code_bg:         Color32,
    pub code_fg:         Color32,
}

impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                title_bg:        Color32::from_rgb(15,  23,  42),   // slate-900
                title_fg:        Color32::from_rgb(203, 213, 225),   // slate-300
                sel_bg:          Color32::from_rgb(30,  58, 138),    // blue-800
                sel_fg:          Color32::from_rgb(147, 197, 253),   // blue-300
                panel_hdr:       Color32::from_rgb(30,  41,  59),    // slate-800
                border:          Color32::from_rgb(51,  65,  85),    // slate-700
                target_fg:       Color32::from_rgb(96, 165, 250),    // blue-400
                ok:              Color32::from_rgb(74, 222, 128),    // green-400
                err:             Color32::from_rgb(248, 113, 113),   // red-400
                warn:            Color32::from_rgb(250, 204,  21),   // yellow-400
                muted:           Color32::from_rgb(148, 163, 184),   // slate-400
                mono_fg:         Color32::from_rgb(96, 165, 250),    // blue-400
                divider:         Color32::from_rgb(51,  65,  85),    // slate-700
                capture_bg:      Color32::from_rgb(55,  42,  10),    // dark amber
                capture_fg:      Color32::from_rgb(252, 211,  77),   // amber-300
                panel_fill:      Color32::from_rgb(15,  23,  42),    // slate-900
                central_bg:      Color32::from_rgb(15,  23,  42),    // slate-900
                top_bar_bg:      Color32::from_rgb(30,  41,  59),    // slate-800
                bottom_bg:       Color32::from_rgb(30,  41,  59),    // slate-800
                preview_bg:      Color32::from_rgb(30,  41,  59),    // slate-800
                info_bg:         Color32::from_rgb(22,  33,  62),    // slate-900+blue
                row_even:        Color32::from_rgb(20,  30,  50),
                row_odd:         Color32::from_rgb(28,  38,  58),
                segment_bg:      Color32::from_rgb(30,  41,  59),    // slate-800
                text:            Color32::from_rgb(226, 232, 240),   // slate-200
                btn_text:        Color32::from_rgb(226, 232, 240),   // slate-200
                hdr_text:        Color32::from_rgb(203, 213, 225),   // slate-300
                val_found_bg:    Color32::from_rgb(20,  45,  25),
                val_notfound_bg: Color32::from_rgb(50,  20,  20),
                fail_step_bg:    Color32::from_rgb(45,  35,  18),
                expected_fg:     Color32::from_rgb(147, 197, 253),   // blue-300
                actual_fg:       Color32::from_rgb(248, 113, 113),   // red-400
                warn_detail_fg:  Color32::from_rgb(251, 191,  36),   // amber-400
                confirm_off:     Color32::from_rgb(71,   85, 105),   // slate-600
                history_fg:      Color32::from_rgb(148, 163, 184),   // slate-400
                capture_border:  Color32::from_rgb(80,  65,  20),
                code_bg:         Color32::from_rgb(30,  41,  59),    // slate-800
                code_fg:         Color32::from_rgb(147, 197, 253),   // blue-300
            }
        } else {
            Self {
                title_bg:        Color32::from_rgb(30,  58, 100),
                title_fg:        Color32::from_rgb(226, 235, 246),
                sel_bg:          Color32::from_rgb(219, 234, 254),
                sel_fg:          Color32::from_rgb(30,  64, 175),
                panel_hdr:       Color32::from_rgb(241, 245, 249),
                border:          Color32::from_rgb(203, 213, 225),
                target_fg:       Color32::from_rgb(30,  64, 175),
                ok:              Color32::from_rgb(22,  163,  74),
                err:             Color32::from_rgb(220,  38,  38),
                warn:            Color32::from_rgb(202, 138,   4),
                muted:           Color32::from_rgb(107, 114, 128),
                mono_fg:         Color32::from_rgb(37,  99, 235),
                divider:         Color32::from_rgb(220, 228, 240),
                capture_bg:      Color32::from_rgb(254, 252, 232),
                capture_fg:      Color32::from_rgb(133,  79,  11),
                panel_fill:      Color32::WHITE,
                central_bg:      Color32::from_gray(250),
                top_bar_bg:      Color32::from_gray(248),
                bottom_bg:       Color32::from_gray(245),
                preview_bg:      Color32::from_gray(252),
                info_bg:         Color32::from_rgb(239, 246, 255),
                row_even:        Color32::from_rgb(252, 252, 255),
                row_odd:         Color32::from_rgb(245, 247, 253),
                segment_bg:      Color32::from_rgb(248, 250, 252),
                text:            Color32::from_gray(35),
                btn_text:        Color32::from_gray(50),
                hdr_text:        Color32::from_gray(70),
                val_found_bg:    Color32::from_rgb(245, 255, 245),
                val_notfound_bg: Color32::from_rgb(255, 245, 245),
                fail_step_bg:    Color32::from_rgb(255, 250, 240),
                expected_fg:     Color32::from_rgb(100, 100, 200),
                actual_fg:       Color32::from_rgb(200, 100, 100),
                warn_detail_fg:  Color32::from_rgb(200, 100,   0),
                confirm_off:     Color32::from_gray(160),
                history_fg:      Color32::from_gray(150),
                capture_border:  Color32::from_rgb(253, 230, 138),
                code_bg:         Color32::from_rgb(243, 244, 246),   // gray-100
                code_fg:         Color32::from_rgb(29,  78, 216),    // blue-700
            }
        }
    }
}
