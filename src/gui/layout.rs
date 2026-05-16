// src/gui/layout.rs
//
// UI 布局组件 - 所有 draw_* 方法

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2};
use std::time::Instant;

use element_selector::core::model::{ElementTab, HierarchyNode, HighlightInfo, Operator, ValidationResult, SegmentValidationResult};
use element_selector::core::xpath;

use super::theme::Theme;
use super::types::*;
use super::helpers::*;
use super::highlight;

/// SelectorApp 的 UI 布局扩展
pub trait LayoutComponents {
    fn draw_titlebar(&self, ui: &mut Ui);
    fn draw_top_bar(&mut self, ui: &mut Ui);
    fn draw_capture_banner(&self, ui: &mut Ui);
    fn draw_bottom_panel(&mut self, ui: &mut Ui);
    fn draw_xpath_preview_content(&mut self, ui: &mut Ui);
    fn draw_element_xpath_content(&mut self, ui: &mut Ui);
    fn draw_window_xpath_content(&mut self, ui: &mut Ui);
    fn draw_left_panel(&mut self, ui: &mut Ui);
    fn draw_element_tree(&mut self, ui: &mut Ui);
    fn draw_window_tree(&mut self, ui: &mut Ui);
    fn draw_right_panel(&mut self, ui: &mut Ui);
    fn draw_element_properties(&mut self, ui: &mut Ui);
    fn draw_window_properties(&mut self, ui: &mut Ui);
    fn draw_validation_details(&self, ui: &mut Ui);
    fn draw_code_dialog(&mut self, ctx: &egui::Context);
}

impl LayoutComponents for crate::gui::app::SelectorApp {
    fn draw_titlebar(&self, ui: &mut Ui) {
        let t = self.theme;
        egui::Panel::top("titlebar")
            .exact_size(32.0)
            .frame(Frame::NONE.fill(t.title_bg))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("🔍  Windows 元素选择器")
                            .color(t.title_fg)
                            .size(13.5)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(12.0);
                        if !self.history.is_empty() {
                            ui.label(
                                RichText::new(format!("历史: {}", self.history.len()))
                                    .color(t.history_fg)
                                    .size(10.5),
                            );
                        }
                    });
                });
            });
    }

    /// 顶部控制栏：名称 + 捕获 + 校验
    fn draw_top_bar(&mut self, ui: &mut Ui) {
        let t = self.theme;
        egui::Panel::top("top_bar")
            .exact_size(44.0)
            .frame(
                Frame::NONE
                    .fill(t.top_bar_bg)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, t.border)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("元素名称:").color(t.muted).size(11.5));
                    ui.add_space(4.0);
                    ui.add(
                        TextEdit::singleline(&mut self.element_name)
                            .desired_width(160.0)
                            .hint_text("输入名称便于速查"),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 【新增】日志窗口开关按钮
                        let log_icon = if self.show_log_panel { "📋 隐藏日志" } else { "📋 显示日志" };
                        if ui.add(
                            egui::Button::new(RichText::new(log_icon).size(12.0))
                                .stroke(Stroke::new(1.0, t.border))
                                .min_size(Vec2::new(100.0, 28.0)),
                        ).on_hover_text("打开/关闭极简优化日志窗口").clicked() {
                            self.show_log_panel = !self.show_log_panel;
                        }
                        
                        ui.add_space(8.0);
                        
                        // 校验按钮
                        let val_label = self.validation.label();
                        let val_color = match &self.validation {
                            ValidationResult::Found { .. } => t.ok,
                            ValidationResult::NotFound | ValidationResult::Error(_) => t.err,
                            _ => t.btn_text,
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&val_label).color(val_color).size(12.0))
                                .stroke(Stroke::new(1.0, val_color))
                                .min_size(Vec2::new(100.0, 28.0)),
                        ).on_hover_text("F7 — 校验当前 XPath 是否有效").clicked() {
                            self.do_validate();
                        }

                        ui.add_space(8.0);

                        // 捕获 / 取消捕获 按钮
                        match &self.capture_state {
                            CaptureState::WaitingClick { deadline } => {
                                let s = deadline.saturating_duration_since(Instant::now()).as_secs();
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new(format!("取消捕获 ({}s)", s))
                                            .color(t.warn)
                                            .size(12.0),
                                    )
                                    .stroke(Stroke::new(1.0, t.warn))
                                    .min_size(Vec2::new(120.0, 28.0)),
                                ).on_hover_text("点击或按 Esc 取消").clicked() {
                                    self.cancel_capture(ui.ctx());
                                }
                            }
                            CaptureState::Capturing => {
                                ui.add(
                                    egui::Button::new(RichText::new("捕获中…").color(t.warn).size(12.0))
                                        .min_size(Vec2::new(100.0, 28.0)),
                                );
                            }
                            CaptureState::Idle => {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("重新捕获 F4")
                                            .color(t.btn_text)
                                            .size(12.0),
                                    )
                                    .min_size(Vec2::new(100.0, 28.0)),
                                ).on_hover_text("F4 — 点击屏幕控件进行捕获").clicked() {
                                    self.start_capture();
                                }
                            }
                        }
                    });
                });
            });
    }

    /// 捕获状态横幅（仅在 WaitingClick 时显示）
    fn draw_capture_banner(&self, ui: &mut Ui) {
        if !matches!(self.capture_state, CaptureState::WaitingClick { .. }) {
            return;
        }
        let t = self.theme;
        egui::Panel::top("capture_banner")
            .exact_size(26.0)
            .frame(
                Frame::NONE
                    .fill(t.capture_bg)
                    .stroke(Stroke::new(1.0, t.capture_border)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("⏳  请点击目标控件 — 按 Esc 或顶栏按钮取消")
                            .color(t.capture_fg)
                            .size(11.5),
                    );
                });
            });
    }

    /// 底部面板：XPath 预览 + 状态 + 确定/取消（合并为单一 Panel）
    fn draw_bottom_panel(&mut self, ui: &mut Ui) {
        let t = self.theme;
        egui::Panel::bottom("bottom_panel")
            .frame(
                Frame::NONE
                    .fill(t.bottom_bg)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, t.border)),
            )
            .show_inside(ui, |ui| {
                // ── XPath 预览区 ─────────────────────────────────────────────
                self.draw_xpath_preview_content(ui);

                ui.add_space(6.0);

                // ── 状态消息 + 操作按钮 ────────────────────────────────────────
                ui.horizontal(|ui| {
                    // 状态消息
                    let msg_color = match &self.validation {
                        ValidationResult::Found { .. }              => t.ok,
                        ValidationResult::NotFound | ValidationResult::Error(_) => t.err,
                        ValidationResult::Running                   => t.warn,
                        _ => t.muted,
                    };
                    ui.label(RichText::new(&self.status_msg).color(msg_color).size(11.5));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 确定按钮（校验通过后变绿）
                        let (confirm_label, confirm_fill) = match &self.validation {
                            ValidationResult::Found { .. } =>
                                ("✔ 确定（已校验）", t.ok),
                            _ =>
                                ("确定", t.confirm_off),
                        };
                        if ui.add(
                            egui::Button::new(
                                RichText::new(confirm_label).color(Color32::WHITE).size(12.0),
                            )
                            .fill(confirm_fill)
                            .min_size(Vec2::new(110.0, 30.0)),
                        ).on_hover_text("保存并关闭（未校验时会先自动校验）").clicked() {
                            self.do_confirm_and_close(ui.ctx());
                        }

                        ui.add_space(6.0);

                        // 取消按钮（强制关闭，不做校验）
                        if ui.add(
                            egui::Button::new(
                                RichText::new("取消").color(t.btn_text).size(12.0),
                            )
                            .min_size(Vec2::new(70.0, 30.0)),
                        ).on_hover_text("放弃本次操作并关闭").clicked() {
                            self.save_to_file();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        ui.add_space(8.0);

                        // 历史记录下拉（右侧辅助功能）
                        if !self.history.is_empty() {
                            egui::ComboBox::from_id_salt("history_combo")
                                .selected_text(format!("历史 ({})", self.history.len()))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    let mut chosen = None;
                                    for (i, h) in self.history.iter().enumerate() {
                                        let label = if h.len() > 40 {
                                            format!("{}…", &h[..40])
                                        } else {
                                            h.clone()
                                        };
                                        if ui.selectable_label(false, label).clicked() {
                                            chosen = Some(i);
                                        }
                                    }
                                    if let Some(i) = chosen {
                                        self.xpath_text      = self.history[i].clone();
                                        self.xpath_source    = XPathSource::Manual;
                                        self.xpath_error     = xpath::lint(&self.xpath_text);
                                        self.validation      = ValidationResult::Idle;
                                    }
                                });
                        }
                    });
                });
            });
    }

    /// XPath 预览内容（根据标签页切换）
    fn draw_xpath_preview_content(&mut self, ui: &mut Ui) {
        let t = self.theme;
        Frame::NONE
            .fill(t.preview_bg)
            .inner_margin(Margin::symmetric(10, 6))
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                match self.active_tab {
                    ElementTab::Element      => self.draw_element_xpath_content(ui),
                    ElementTab::WindowElement => self.draw_window_xpath_content(ui),
                }
            });
    }

    /// 元素模式：XPath 预览内容
    fn draw_element_xpath_content(&mut self, ui: &mut Ui) {
        let t = self.theme;
        ui.horizontal(|ui| {
            ui.label(RichText::new("元素 XPath:").color(t.muted).size(11.0));
            ui.add_space(4.0);

            if ui.small_button("复制").on_hover_text("复制元素 XPath").clicked() {
                ui.ctx().copy_text(self.element_xpath.clone());
                self.status_msg = "元素 XPath 已复制到剪贴板".to_string();
            }

            if ui.small_button("智能优化").on_hover_text("自动优化 XPath，移除动态属性，使用锚点定位").clicked() {
                self.do_optimize();
            }
            
            // 极简优化按钮（根据状态显示不同文本）
            use std::sync::atomic::Ordering;
            let is_optimizing = self.optimization_in_progress.load(Ordering::SeqCst);
            let (btn_text, btn_tooltip) = if is_optimizing {
                ("❌ 取消", "取消正在进行的极简优化")
            } else {
                ("🎯 极简优化", "通过尝试验证逐个移除属性，得到最简 XPath")
            };
            
            if ui.small_button(btn_text).on_hover_text(btn_tooltip).clicked() {
                if is_optimizing {
                    self.cancel_minimal_optimize();
                } else {
                    self.do_minimal_optimize();
                }
            }

            if ui.small_button("生成 TS 代码").on_hover_text("生成 TypeScript SDK 选择器代码").clicked() {
                self.open_code_dialog();
            }

            // 重置：仅在 Optimized 或 Manual 状态下显示
            if !self.xpath_source.is_auto() {
                if ui.small_button("重置").on_hover_text("恢复为自动生成的 XPath").clicked() {
                    self.xpath_source = XPathSource::AutoGenerated;
                    self.rebuild_xpath();
                }
            }

            // 优化摘要标签
            if let Some(summary) = self.xpath_source.optimization_summary() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "已优化：移除 {} / 简化 {} {}",
                        summary.removed_dynamic_attrs,
                        summary.simplified_attrs,
                        if summary.used_anchor { "· 锚点" } else { "" }
                    ))
                    .color(t.ok)
                    .size(10.0),
                );
            }

            // 手动编辑标签
            if matches!(self.xpath_source, XPathSource::Manual) {
                ui.add_space(4.0);
                ui.label(RichText::new("[手动编辑]").color(t.warn).size(10.0));
            }

            if let Some(err) = &self.xpath_error {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("⚠ {}", err)).color(t.err).size(10.5));
            }
        });

        let edit_resp = ui.add(
            TextEdit::multiline(&mut self.element_xpath)
                .font(egui::TextStyle::Monospace)
                .desired_rows(2)
                .desired_width(ui.available_width())
                .hint_text("/ControlType[@Attr='val']")
                .text_color(t.mono_fg),
        );
        if edit_resp.changed() {
            self.xpath_source = XPathSource::Manual;
            self.xpath_text   = format!("{}, {}", self.window_selector, self.element_xpath);
            self.xpath_error  = xpath::lint(&self.xpath_text);
            self.validation   = ValidationResult::Idle;
        }
    }

    /// 窗口元素模式：XPath 预览内容
    fn draw_window_xpath_content(&mut self, ui: &mut Ui) {
        let t = self.theme;
        ui.horizontal(|ui| {
            ui.label(RichText::new("窗口选择器:").color(t.muted).size(11.0));
            ui.add_space(4.0);

            if ui.small_button("复制").on_hover_text("复制窗口选择器").clicked() {
                ui.ctx().copy_text(self.window_selector.clone());
                self.status_msg = "窗口选择器已复制到剪贴板".to_string();
            }

            if self.custom_window_xpath {
                if ui.small_button("重置").on_hover_text("回到自动生成的窗口选择器").clicked() {
                    self.custom_window_xpath = false;
                    self.init_window_filters();
                    self.rebuild_xpath();
                }
                ui.label(RichText::new("[自定义]").color(t.warn).size(10.0));
            }
        });

        let edit_resp = ui.add(
            TextEdit::multiline(&mut self.window_selector)
                .font(egui::TextStyle::Monospace)
                .desired_rows(2)
                .desired_width(ui.available_width())
                .hint_text("Window[@Name='...' and @ClassName='...']")
                .text_color(t.mono_fg),
        );
        if edit_resp.changed() {
            self.custom_window_xpath = true;
            self.xpath_text  = format!("{}, {}", self.window_selector, self.element_xpath);
            self.xpath_error = xpath::lint(&self.xpath_text);
            self.validation  = ValidationResult::Idle;
        }
    }

    // ── Left panel ────────────────────────────────────────────────────────────

    fn draw_left_panel(&mut self, ui: &mut Ui) {
        match self.active_tab {
            ElementTab::Element       => self.draw_element_tree(ui),
            ElementTab::WindowElement => self.draw_window_tree(ui),
        }
    }

    fn draw_element_tree(&mut self, ui: &mut Ui) {
        let t = self.theme;
        panel_header(ui, "📂  元素层级结构", t);
        ui.add_space(2.0);

        if self.node_expanded.len() != self.hierarchy.len() {
            self.node_expanded.resize(self.hierarchy.len(), true);
        }

        ScrollArea::vertical()
            .id_salt("tree_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let n = self.hierarchy.len();
                if n == 0 {
                    ui.label(RichText::new("尚未捕获元素").color(t.muted).italics());
                    return;
                }

                ui.spacing_mut().item_spacing.y = 0.0;

                let validation_segments     = self.detailed_validation.as_ref().map(|d| &d.segments);
                let show_validation_details = self.config.show_validation_details;
                let validation_result        = Some(&self.validation);

                let included_changed = draw_tree_flat(
                    ui,
                    &mut self.hierarchy,
                    &mut self.selected_node,
                    validation_segments,
                    show_validation_details,
                    validation_result,
                    t,
                );

                if included_changed {
                    // 无论之前是什么状态，用户手动调整后都切换到自动生成模式
                    // 这样用户可以自由调整，不会被智能优化覆盖
                    self.xpath_source = XPathSource::AutoGenerated;
                    self.rebuild_xpath();
                }
                ui.add_space(12.0);
            });
    }

    fn draw_window_tree(&mut self, ui: &mut Ui) {
        let t = self.theme;
        panel_header(ui, "🪟  窗口信息", t);
        ui.add_space(4.0);

        if let Some(ref win) = self.window_info {
            egui::Frame::NONE
                .fill(t.info_bg)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        prop_row(ui, "标题",  &win.title, t);
                        prop_row(ui, "类名",  if win.class_name.is_empty()   { "(空)" } else { &win.class_name }, t);
                        prop_row(ui, "进程名", if win.process_name.is_empty() { "(空)" } else { &win.process_name }, t);
                        prop_row(ui, "进程ID", &win.process_id.to_string(), t);
                    });
                });
        } else {
            ui.label(RichText::new("尚未选择窗口").color(t.muted).italics());
            ui.add_space(4.0);
            ui.label(RichText::new("请先捕获元素").color(t.muted).size(10.5));
        }
    }

    // ── Right panel ───────────────────────────────────────────────────────────

    fn draw_right_panel(&mut self, ui: &mut Ui) {
        match self.active_tab {
            ElementTab::Element       => self.draw_element_properties(ui),
            ElementTab::WindowElement => self.draw_window_properties(ui),
        }
        self.draw_validation_details(ui);
    }

    fn draw_element_properties(&mut self, ui: &mut Ui) {
        let t = self.theme;
        panel_header(ui, "⚙  元素属性", t);

        ScrollArea::vertical()
            .id_salt("prop_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let Some(sel_idx) = self.selected_node else {
                    ui.add_space(24.0);
                    ui.label(RichText::new("← 点击左侧节点查看属性").color(t.muted).italics());
                    return;
                };
                if sel_idx >= self.hierarchy.len() { return; }

                // 节点摘要
                {
                    let node = &self.hierarchy[sel_idx];
                    egui::Frame::NONE
                        .fill(t.info_bg)
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("控件类型").color(t.muted).size(11.0));
                                ui.add_space(6.0);
                                ui.label(RichText::new(&node.control_type).color(t.target_fg).strong().size(13.0));
                                if node.rect.width > 0 {
                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "{}×{}  @({},{})",
                                            node.rect.width, node.rect.height,
                                            node.rect.x, node.rect.y,
                                        ))
                                        .color(t.muted).size(10.5),
                                    );
                                }
                                if node.process_id > 0 {
                                    ui.label(
                                        RichText::new(format!("pid:{}", node.process_id))
                                            .color(t.muted).size(10.5),
                                    );
                                }
                            });
                        });
                }

                ui.add_space(6.0);

                // 属性列表表头
                egui::Frame::NONE
                    .fill(t.panel_hdr)
                    .inner_margin(Margin::symmetric(4, 2))
                    .stroke(Stroke::new(0.5, t.border))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            col_label(ui, "属性名", 100.0, t);
                            col_label(ui, "运算符",  80.0, t);
                            col_label(ui, "值",       0.0, t);
                        });
                    });

                ui.add_space(2.0);

                let filter_count = self.hierarchy[sel_idx].filters.len();
                let mut dirty = false;
                for fi in 0..filter_count {
                    let row_color = if fi % 2 == 0 { t.row_even } else { t.row_odd };

                    egui::Frame::NONE
                        .fill(row_color)
                        .inner_margin(Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            let filter = &mut self.hierarchy[sel_idx].filters[fi];
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut filter.enabled, "").changed() { dirty = true; }

                                ui.add_sized(
                                    Vec2::new(100.0, 20.0),
                                    egui::Label::new(RichText::new(&filter.name.clone()).size(12.0)),
                                );

                                let old_op = filter.operator.clone();
                                egui::ComboBox::from_id_salt(format!("op_{}_{}", sel_idx, fi))
                                    .selected_text(filter.operator.label())
                                    .width(76.0)
                                    .show_ui(ui, |ui| {
                                        for op in Operator::all() {
                                            ui.selectable_value(&mut filter.operator, op.clone(), op.label());
                                        }
                                    });
                                if filter.operator != old_op { dirty = true; }

                                if ui.add(
                                    TextEdit::singleline(&mut filter.value)
                                        .desired_width(ui.available_width() - 4.0)
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("—"),
                                ).changed() {
                                    filter.enabled = !filter.value.is_empty();
                                    dirty = true;
                                }
                            });
                        });
                }
                if dirty {
                    self.xpath_source = XPathSource::AutoGenerated;
                    self.rebuild_xpath();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 本节点 XPath 片段预览
                ui.label(RichText::new("本节点 XPath 片段:").color(t.muted).size(11.0));
                ui.add_space(2.0);
                let seg = self.hierarchy[sel_idx].xpath_segment();
                egui::Frame::NONE
                    .fill(t.segment_bg)
                    .stroke(Stroke::new(0.5, t.border))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&seg)
                                .font(egui::FontId::monospace(11.0))
                                .color(t.mono_fg),
                        );
                    });

                // 包含/排除切换
                ui.add_space(8.0);
                let included    = self.hierarchy[sel_idx].included;
                let toggle_txt  = if included { "⊖ 从 XPath 中排除此节点" } else { "⊕ 将此节点加入 XPath" };
                let toggle_color = if included { t.err } else { t.ok };
                if ui.add(action_btn(toggle_txt, 0.0, toggle_color)).clicked() {
                    self.hierarchy[sel_idx].included = !included;
                    self.xpath_source = XPathSource::AutoGenerated;
                    self.rebuild_xpath();
                }
                ui.add_space(8.0);
            });
    }

    fn draw_window_properties(&mut self, ui: &mut Ui) {
        let t = self.theme;
        panel_header(ui, "⚙  窗口属性过滤器", t);

        ScrollArea::vertical()
            .id_salt("window_prop_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // 仅在未初始化时才初始化（不在每帧 draw 里判断 is_empty）
                if !self.window_filters_initialized && self.window_info.is_some() {
                    self.init_window_filters();
                }

                if self.window_info.is_none() {
                    ui.add_space(24.0);
                    ui.label(RichText::new("← 请先捕获元素以获取窗口信息").color(t.muted).italics());
                    return;
                }

                egui::Frame::NONE
                    .fill(t.panel_hdr)
                    .inner_margin(Margin::symmetric(4, 2))
                    .stroke(Stroke::new(0.5, t.border))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            col_label(ui, "属性名", 100.0, t);
                            col_label(ui, "运算符",  80.0, t);
                            col_label(ui, "值",       0.0, t);
                        });
                    });

                ui.add_space(2.0);

                let filter_count = self.window_filters.len();
                let mut dirty = false;
                for fi in 0..filter_count {
                    let row_color = if fi % 2 == 0 { t.row_even } else { t.row_odd };

                    egui::Frame::NONE
                        .fill(row_color)
                        .inner_margin(Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            let filter = &mut self.window_filters[fi];
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut filter.enabled, "").changed() { dirty = true; }

                                ui.add_sized(
                                    Vec2::new(100.0, 20.0),
                                    egui::Label::new(RichText::new(&filter.name.clone()).size(12.0)),
                                );

                                let old_op = filter.operator.clone();
                                egui::ComboBox::from_id_salt(format!("win_op_{}", fi))
                                    .selected_text(filter.operator.label())
                                    .width(76.0)
                                    .show_ui(ui, |ui| {
                                        for op in Operator::all() {
                                            ui.selectable_value(&mut filter.operator, op.clone(), op.label());
                                        }
                                    });
                                if filter.operator != old_op { dirty = true; }

                                if ui.add(
                                    TextEdit::singleline(&mut filter.value)
                                        .desired_width(ui.available_width() - 4.0)
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("—"),
                                ).changed() {
                                    filter.enabled = !filter.value.is_empty();
                                    dirty = true;
                                }
                            });
                        });
                }

                if dirty {
                    self.custom_window_xpath = true;
                    self.rebuild_window_selector();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                ui.label(RichText::new("窗口选择器:").color(t.muted).size(11.0));
                ui.add_space(2.0);
                egui::Frame::NONE
                    .fill(t.segment_bg)
                    .stroke(Stroke::new(0.5, t.border))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&self.window_selector)
                                .font(egui::FontId::monospace(11.0))
                                .color(t.mono_fg),
                        );
                    });
            });
    }

    /// 校验详情（失败时展示逐段分析）
    fn draw_validation_details(&self, ui: &mut Ui) {
        let Some(detail) = &self.detailed_validation else { return; };
        let t = self.theme;
    
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
    
        panel_header(ui, "🔍 校验结果", t);
        ui.add_space(4.0);
    
        let (status_color, status_text) = match &detail.overall {
            ValidationResult::Found { count, .. } =>
                (t.ok, format!("✓ 通过 — 找到 {} 个元素", count)),
            ValidationResult::NotFound =>
                (t.err, "✗ 未找到匹配元素".to_string()),
            ValidationResult::Error(e) =>
                (t.warn_detail_fg, format!("⚠ 错误: {}", e)),
            _ =>
                (t.muted, "未校验".to_string()),
        };
    
        let frame_fill = if detail.overall == ValidationResult::NotFound {
            t.val_notfound_bg
        } else {
            t.val_found_bg
        };
    
        egui::Frame::NONE
            .fill(frame_fill)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.label(RichText::new(status_text).color(status_color).strong().size(13.0));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("用时: {}ms", detail.total_duration_ms)).color(t.muted).size(11.0));
            });
    
        if detail.overall != ValidationResult::NotFound { return; }
    
        ui.add_space(8.0);
        ui.label(RichText::new("失败步骤分析:").color(t.muted).size(11.0));
        ui.add_space(4.0);
    
        for seg in &detail.segments {
            if seg.matched || seg.match_count > 0 { continue; }
    
            egui::Frame::NONE
                .fill(t.fail_step_bg)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("第 {} 步失败:", seg.segment_index + 1))
                            .color(t.warn_detail_fg)
                            .size(11.0),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(&seg.segment_text)
                            .font(egui::FontId::monospace(10.0))
                            .color(t.mono_fg),
                    );
    
                    if !seg.predicate_failures.is_empty() {
                        ui.add_space(4.0);
                        for pf in &seg.predicate_failures {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{}: ", pf.attr_name)).color(t.muted).size(10.0));
                                ui.label(RichText::new(format!("期望 '{}'", pf.expected_value)).color(t.expected_fg).size(10.0));
                                if let Some(ref actual) = pf.actual_value {
                                    ui.label(RichText::new(" vs ").color(t.muted).size(10.0));
                                    ui.label(RichText::new(format!("实际 '{}'", actual)).color(t.actual_fg).size(10.0));
                                }
                            });
                            ui.label(RichText::new(&pf.reason).color(t.muted).size(10.0));
                        }
                    } else {
                        ui.label(RichText::new("• 属性值在验证时可能已变化").color(t.muted).size(10.0));
                        ui.label(RichText::new("• 元素结构在捕获后可能已变化").color(t.muted).size(10.0));
                    }
                });
            ui.add_space(4.0);
        }
    
        ui.add_space(8.0);
        ui.label(RichText::new("建议:").color(t.muted).size(11.0));
        ui.add_space(4.0);
        ui.label(RichText::new("1. 点击【智能优化】按钮，移除动态属性").color(t.muted).size(10.0));
        ui.label(RichText::new("2. 检查动态类名是否在捕获后发生了变化").color(t.muted).size(10.0));
        ui.label(RichText::new("3. 重新捕获元素，确保元素状态稳定").color(t.muted).size(10.0));
    }
    
    /// TypeScript 代码生成对话框
    fn draw_code_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_code_dialog {
            return;
        }
        
        let t = self.theme;
        let mut should_close = false;
        let mut new_format: Option<CodeFormat> = None;
        let mut should_copy = false;
        
        // 使用局部变量避免借用冲突
        let mut show_dialog = self.show_code_dialog;
        let generated_code = &mut self.generated_ts_code;
        let current_format = self.code_format.clone();
        
        egui::Window::new("TypeScript 代码生成")
            .open(&mut show_dialog)
            .default_size([600.0, 400.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                // 标题说明
                ui.label(
                    RichText::new("生成的 TypeScript SDK 选择器代码：")
                        .color(t.text)
                        .size(12.0),
                );
                ui.add_space(8.0);
                
                // 代码编辑区
                egui::Frame::NONE
                    .fill(t.code_bg)
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(CornerRadius::same(4))
                    .show(ui, |ui| {
                        let _edit_resp = ui.add(
                            TextEdit::multiline(generated_code)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(8)
                                .desired_width(ui.available_width())
                                .text_color(t.code_fg),
                        );
                    });
                
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                
                // 底部操作按钮和格式切换
                ui.horizontal(|ui| {
                    // 左侧：格式切换单选按钮
                    ui.label(RichText::new("代码格式:").color(t.muted).size(11.0));
                    ui.add_space(8.0);
                    
                    if ui.selectable_label(current_format == CodeFormat::FullChain, "完整示例")
                        .on_hover_text("适合从头开始编写新脚本\n包含 SDK 初始化和完整流程")
                        .clicked() {
                        new_format = Some(CodeFormat::FullChain);
                    }
                    
                    if ui.selectable_label(current_format == CodeFormat::ParamsObject, "参数对象")
                        .on_hover_text("适合在已有代码中插入新步骤\n生成 windowSelector 和 xpath 变量")
                        .clicked() {
                        new_format = Some(CodeFormat::ParamsObject);
                    }
                    
                    if ui.selectable_label(current_format == CodeFormat::XPathOnly, "仅 XPath")
                        .on_hover_text("快速复制 XPath 字符串\n//Document")
                        .clicked() {
                        new_format = Some(CodeFormat::XPathOnly);
                    }
                    
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 关闭按钮
                        if ui.button("关闭").clicked() {
                            should_close = true;
                        }
                        
                        ui.add_space(8.0);
                        
                        // 复制按钮
                        if ui.button("复制到剪贴板").clicked() {
                            should_copy = true;
                        }
                    });
                });
            });
        
        // 更新 show_code_dialog 状态
        self.show_code_dialog = show_dialog;
        
        // 处理格式切换
        if let Some(format) = new_format {
            if self.code_format != format {
                self.code_format = format;
                self.generated_ts_code = self.generate_typescript_code();
            }
        }
        
        // 处理复制
        if should_copy {
            self.copy_code_to_clipboard(ctx);
            self.status_msg = "TypeScript 代码已复制到剪贴板".to_string();
        }
        
        // 处理关闭
        if should_close {
            self.close_code_dialog();
        }
    }
}

// ─── 独立的辅助函数（不在 trait 中）─────────────────────────────────────────────

/// 平面列表形式的元素层级树
pub fn draw_tree_flat(
    ui: &mut Ui,
    hierarchy: &mut Vec<HierarchyNode>,
    selected_node: &mut Option<usize>,
    validation_segments: Option<&Vec<SegmentValidationResult>>,
    show_validation_details: bool,
    validation_result: Option<&ValidationResult>,
    t: Theme,
) -> bool {
    let n = hierarchy.len();
    let mut included_changed = false;

    for idx in 0..n {
        let is_target = idx == n - 1;
        let is_sel    = *selected_node == Some(idx);

        let label_text   = hierarchy[idx].tree_label();
        let node_included = hierarchy[idx].included;
        let icon = if is_target { "🎯" } else { "" };
        let label_color = if is_target        { t.target_fg }
                          else if is_sel       { t.sel_fg }
                          else if !node_included { t.muted }
                          else                  { t.text };

        let validation_marker = if show_validation_details {
            validation_segments
                .and_then(|segs| segs.get(idx))
                .map(|seg| {
                    if seg.matched {
                        format!(" ✅ {}ms", seg.duration_ms)
                    } else {
                        format!(" ❌ {}ms", seg.duration_ms)
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };

        let header_text = RichText::new(format!("{} {}{}", icon, label_text, validation_marker))
            .size(12.0)
            .color(label_color)
            .strong_if(is_sel || is_target);

        let row_bg = if is_sel { t.sel_bg } else { Color32::TRANSPARENT };

        egui::Frame::NONE
            .fill(row_bg)
            .inner_margin(Margin::symmetric(2, 1))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let cb_resp = ui.checkbox(&mut hierarchy[idx].included, "");
                    if cb_resp.changed() { included_changed = true; }
                    cb_resp.on_hover_text(if hierarchy[idx].included {
                        "已包含在 XPath 中 — 取消勾选将排除此节点"
                    } else {
                        "已排除 — 勾选将包含在 XPath 中"
                    });

                    let resp = ui.add(
                        egui::Label::new(header_text)
                            .sense(Sense::click_and_drag())
                            .truncate(),
                    );
                    if resp.clicked() {
                        *selected_node = Some(idx);
                    }

                    resp.context_menu(|ui| {
                        node_context_menu(ui, hierarchy, idx, selected_node, validation_result);
                    });

                    resp.on_hover_ui(|ui| {
                        node_tooltip(ui, &hierarchy[idx], t);
                    });
                });
            });
    }

    included_changed
}

/// 右键菜单：只提供操作，不放提示语
pub fn node_context_menu(
    ui: &mut Ui,
    hierarchy: &mut Vec<HierarchyNode>,
    idx: usize,
    selected_node: &mut Option<usize>,
    validation_result: Option<&ValidationResult>,
) {
    let included = hierarchy[idx].included;

    if ui.button(if included { "⊖ 从 XPath 中排除此节点" } else { "⊕ 将此节点加入 XPath" }).clicked() {
        hierarchy[idx].included = !included;
        ui.close();
    }

    ui.separator();

    // 高亮逻辑改进：根据校验状态决定使用哪个 rect
    let highlight_label = match validation_result {
        Some(ValidationResult::Found { .. }) => "🔍 高亮显示此元素（校验位置）",
        Some(ValidationResult::NotFound) => "🔍 高亮显示此元素（捕获位置⚠）",
        Some(ValidationResult::Error(_)) => "🔍 高亮显示此元素（捕获位置⚠）",
        _ => "🔍 高亮显示此元素（捕获位置）",
    };        
    if ui.button(highlight_label).clicked() {
        // 校验成功时，优先使用校验结果的 rect
        let rect_to_use = match validation_result {
            Some(ValidationResult::Found { first_rect: Some(r), .. }) => r.clone(),
            _ => hierarchy[idx].rect.clone(),
        };
        let info = HighlightInfo::new(rect_to_use, &hierarchy[idx].control_type);
        highlight::flash_with_info(&info, 1500);
        ui.close();
    }

    if ui.button("📋 复制此节点 XPath 片段").clicked() {
        ui.ctx().copy_text(hierarchy[idx].xpath_segment());
        ui.close();
    }

    ui.separator();

    if ui.button("选中此节点").clicked() {
        *selected_node = Some(idx);
        ui.close();
    }
}
