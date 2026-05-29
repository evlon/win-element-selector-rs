// src/api/mouse.rs
//
// 鼠标操作 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};

use super::types::{
    MouseMoveRequest, MouseMoveResponse, MouseMoveOptions,
    MouseClickRequest, MouseClickResponse, MouseClickOptions,
    MouseScrollRequest, MouseScrollResponse, MouseScrollOptions,
    MouseHoverRequest, MouseHoverResponse, MouseHoverOptions,
    MouseDragRequest, MouseDragResponse, MouseDragOptions,
    MouseScrollDetectRequest, MouseScrollDetectResponse,
    Point,
};
use super::super::mouse_control;
use super::idle_motion::with_auto_pause;

/// POST /api/mouse/move
/// 拟人化移动鼠标到目标坐标
pub async fn move_mouse(body: web::Json<MouseMoveRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseMoveOptions::default());

    info!(
        "API: /api/mouse/move target=({}, {}) humanize={} trajectory={} duration={}ms",
        request.target.x, request.target.y,
        options.humanize, options.trajectory, options.duration
    );

    // 使用 with_auto_pause 包装，自动暂停空闲移动
    with_auto_pause(|| async {
        // 获取当前鼠标位置
        let start_point = mouse_control::get_cursor_position();

        // 执行移动
        let result = if options.humanize {
            mouse_control::humanized_move(
                start_point,
                request.target,
                options.duration,
                &options.trajectory,
            )
        } else {
            mouse_control::linear_move(start_point, request.target)
        };

        match result {
            Ok(_) => {
                info!("Mouse moved successfully");
                HttpResponse::Ok().json(MouseMoveResponse {
                    success: true,
                    start_point,
                    end_point: request.target,
                    duration_ms: options.duration,
                    error: None,
                })
            }
            Err(e) => {
                warn!("Mouse move failed: {}", e);
                HttpResponse::Ok().json(MouseMoveResponse {
                    success: false,
                    start_point,
                    end_point: request.target,
                    duration_ms: 0,
                    error: Some(e.to_string()),
                })
            }
        }
    }).await
}

/// POST /api/mouse/click
/// 拟人化点击元素
pub async fn click_mouse(body: web::Json<MouseClickRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseClickOptions::default());

    info!(
        "API: /api/mouse/click window='{}' element='{}' humanize={} random_range={}",
        match &request.window {
            super::types::WindowSelectorOrString::String(s) => s.as_str(),
            super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or(""),
        },
        request.element,
        options.humanize,
        options.random_range
    );

    // Step 1: 构建窗口选择器
    let window_selector = build_window_selector(&request.window);
    let window_selector_for_click = window_selector.clone();  // 克隆一份用于点击

    // Step 2: 获取元素坐标
    let element = request.element.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector,
            &element,
            &[],  // API层无 hierarchy 数据，layers 为空
        )
    })
    .await;

    match element_result {
        Ok(detailed_result) => {
            use super::super::model::ValidationResult;

            match &detailed_result.overall {
                ValidationResult::Found { first_rect, .. } => {
                    if let Some(rect) = first_rect {
                        let rect_api: super::types::Rect = rect.clone().into();
                        let _center = rect_api.center();

                        // 计算随机点击点
                        let click_point = calculate_random_click_point(&rect_api, options.random_range, &options.click_area);

                        // Step 3: 使用 with_auto_pause 执行拟人化移动和点击
                        let click_point_copy = click_point;
                        let options_copy = options.clone();

                        with_auto_pause(|| async {
                            // 确保窗口在前台
                            info!("Activating window before click: {}", window_selector_for_click);
                            let activated = super::super::core::uia::activate_window_by_selector(&window_selector_for_click);
                            if !activated {
                                warn!("Failed to activate window, but continuing...");
                                // 继续尝试点击，可能窗口已经在前台
                            }

                            // 短暂等待让窗口激活
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                            let move_start = std::time::Instant::now();
                            let start_point = mouse_control::get_cursor_position();

                            let move_result = if options_copy.humanize {
                                mouse_control::humanized_move(
                                    start_point,
                                    click_point_copy,
                                    600,
                                    "bezier",
                                )
                            } else {
                                mouse_control::linear_move(start_point, click_point_copy)
                            };

                            let move_duration = move_start.elapsed();
                            info!("Mouse move completed in {:?}", move_duration);

                            if let Err(e) = move_result {
                                warn!("Move to click position failed: {}", e);
                                return HttpResponse::Ok().json(MouseClickResponse {
                                    success: false,
                                    click_point: click_point_copy,
                                    element: None,
                                    error: Some(format!("移动失败: {}", e)),
                                });
                            }

                            // 点击前停顿
                            if options_copy.pause_before > 0 {
                                tokio::time::sleep(tokio::time::Duration::from_millis(options_copy.pause_before)).await;
                            }

                            // 执行点击
                            let click_start = std::time::Instant::now();
                            let click_result = if options_copy.button == "right" {
                                mouse_control::right_click_at(click_point_copy)
                            } else {
                                mouse_control::click_at(click_point_copy)
                            };
                            let click_duration = click_start.elapsed();
                            info!("Mouse click completed in {:?}", click_duration);

                            // 点击后停顿
                            if options_copy.pause_after > 0 {
                                tokio::time::sleep(tokio::time::Duration::from_millis(options_copy.pause_after)).await;
                            }

                            match click_result {
                                Ok(_) => {
                                    info!("Click executed successfully at ({}, {})", click_point_copy.x, click_point_copy.y);

                                    // 点击留痕：在点击位置显示红色圆点标记
                                    if options_copy.mark_click {
                                        let mark_timeout = options_copy.mark_timeout;
                                        crate::highlight::flash_point(
                                            click_point_copy.x,
                                            click_point_copy.y,
                                            mark_timeout,
                                        );
                                    }

                                    HttpResponse::Ok().json(MouseClickResponse {
                                        success: true,
                                        click_point: click_point_copy,
                                        element: Some(super::types::ClickedElement {
                                            control_type: "Element".to_string(),
                                            name: String::new(),
                                        }),
                                        error: None,
                                    })
                                }
                                Err(e) => {
                                    warn!("Click failed: {}", e);
                                    HttpResponse::Ok().json(MouseClickResponse {
                                        success: false,
                                        click_point: click_point_copy,
                                        element: None,
                                        error: Some(format!("点击失败: {}", e)),
                                    })
                                }
                            }
                        }).await
                    } else {
                        HttpResponse::Ok().json(MouseClickResponse {
                            success: false,
                            click_point: Point::new(0, 0),
                            element: None,
                            error: Some("元素坐标获取失败".to_string()),
                        })
                    }
                }
                ValidationResult::NotFound => {
                    HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        error: Some(format!(
                            "未找到匹配元素 (耗时 {}ms)",
                            detailed_result.total_duration_ms
                        )),
                    })
                }
                ValidationResult::Error(e) => {
                    HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        error: Some(e.clone()),
                    })
                }
                _ => {
                    HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        error: Some("校验状态未知".to_string()),
                    })
                }
            }
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// 构建窗口选择器字符串
fn build_window_selector(selector: &super::types::WindowSelectorOrString) -> String {
    match selector {
        super::types::WindowSelectorOrString::String(s) => s.clone(),
        super::types::WindowSelectorOrString::Object(obj) => {
            let mut predicates: Vec<String> = Vec::new();

            if let Some(ref title) = obj.title {
                predicates.push(format!("@Name='{}'", title));
            }
            if let Some(ref class_name) = obj.class_name {
                predicates.push(format!("@ClassName='{}'", class_name));
            }
            if let Some(ref process_name) = obj.process_name {
                predicates.push(format!("@ProcessName='{}'", process_name));
            }

            if predicates.is_empty() {
                "Window".to_string()
            } else {
                format!("Window[{}]", predicates.join(" and "))
            }
        }
    }
}

/// 计算随机点击点
fn calculate_random_click_point(rect: &super::types::Rect, range_percent: f32, click_area: &Option<super::types::ClickArea>) -> Point {
    use rand::Rng;

    // 计算有效区域边界
    let (eff_left, eff_right, eff_top, eff_bottom) = if let Some(ref area) = click_area {
        let left = area.left.unwrap_or(0.0);
        let right = area.right.unwrap_or(0.0);
        let top = area.top.unwrap_or(0.0);
        let bottom = area.bottom.unwrap_or(0.0);

        let eff_left = rect.x as f32 + rect.width as f32 * left;
        let eff_right = rect.x as f32 + rect.width as f32 * (1.0 - right);
        let eff_top = rect.y as f32 + rect.height as f32 * top;
        let eff_bottom = rect.y as f32 + rect.height as f32 * (1.0 - bottom);
        (eff_left, eff_right, eff_top, eff_bottom)
    } else {
        // 无 clickArea 时以中心为基准，保持原有行为
        let cx = rect.center().x as f32;
        let cy = rect.center().y as f32;
        let hw = rect.width as f32 * range_percent / 2.0;
        let hh = rect.height as f32 * range_percent / 2.0;
        (cx - hw, cx + hw, cy - hh, cy + hh)
    };

    let center_x = (eff_left + eff_right) / 2.0;
    let center_y = (eff_top + eff_bottom) / 2.0;
    let half_range_w = (eff_right - eff_left) * range_percent / 2.0;
    let half_range_h = (eff_bottom - eff_top) * range_percent / 2.0;

    let mut rng = rand::thread_rng();

    let offset_x = if half_range_w > 0.0 {
        rng.gen_range(-half_range_w..half_range_w) as i32
    } else {
        0
    };
    let offset_y = if half_range_h > 0.0 {
        rng.gen_range(-half_range_h..half_range_h) as i32
    } else {
        0
    };

    Point::new(center_x as i32 + offset_x, center_y as i32 + offset_y)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/scroll
/// 滚动鼠标滚轮，边滚边检测 wait xpath
pub async fn scroll_mouse(body: web::Json<MouseScrollRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseScrollOptions::default());
    let times = options.times.unwrap_or(3);
    let delta = options.delta.unwrap_or(120);
    let timeout_ms = options.timeout.unwrap_or(5000);
    let auto_delta = options.auto_delta.unwrap_or(false);
    let delta_factor = options.delta_factor.unwrap_or(0.8);
    let wait_visible = options.wait_mode.as_deref() == Some("visible");
    let scroll_to_center = options.scroll_to_center.unwrap_or(true);
    let scroll_to_center_adjust_times = options.scroll_to_center_adjust_times.unwrap_or(5);
    // 优先使用传入的窗口选择器，否则回退到通用 "Window"
    let window_selector = request.window.as_deref().unwrap_or("Window").to_string();
    let window_selector_for_wait = window_selector.clone();

    info!(
        "API: /api/mouse/scroll window='{}' element='{}' times={} delta={} auto_delta={} delta_factor={} wait={:?} wait_mode={:?} timeout={}ms",
        window_selector, request.element, times, delta, auto_delta, delta_factor, options.wait, options.wait_mode, timeout_ms
    );

    // Step 1: 获取元素坐标
    let element_for_query = request.element.clone();
    let window_selector_for_element = window_selector.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector_for_element,
            &element_for_query,
            &[],
        )
    })
    .await;

    let element_result = match element_result {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                scrolled_to_end: false,
                error: Some(format!("内部错误: {}", e)),
            });
        }
    };

    use super::super::model::ValidationResult;

    let (scroll_point, container_rect) = match &element_result.overall {
        ValidationResult::Found { first_rect, .. } => {
            if let Some(rect) = first_rect {
                let rect_api: super::types::Rect = rect.clone().into();
                let point = Point::new(
                    rect_api.x as i32 + rect_api.width as i32 / 2,
                    rect_api.y as i32 + rect_api.height as i32 / 2,
                );
                (point, Some(rect_api))
            } else {
                return HttpResponse::Ok().json(MouseScrollResponse {
                    success: false,
                    scrolled: 0,
                    target_found: false,
                    target_rect: None,
                    scrolled_to_end: false,
                    error: Some("元素坐标获取失败".to_string()),
                });
            }
        }
        ValidationResult::NotFound => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                scrolled_to_end: false,
                error: Some(format!("未找到元素: {}", request.element)),
            });
        }
        ValidationResult::Error(e) => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                scrolled_to_end: false,
                error: Some(e.clone()),
            });
        }
        _ => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                scrolled_to_end: false,
                error: Some("校验状态未知".to_string()),
            });
        }
    };

    // 用滚动容器的 rect 计算方向性视口目标 Y
    // 下滚(delta<0, 元素从下方进入): 目标在视口 70% 处，留顶部余量防过冲
    // 上滚(delta>0, 元素从上方进入): 目标在视口 30% 处，留底部余量防过冲
    let viewport_center_y: Option<i32> = if scroll_to_center {
        container_rect.as_ref().map(|r| {
            let target = if delta < 0 {
                // 向下滚动 → 目标偏下(70%)，过冲时元素仍在上半区可见
                r.y + (r.height as f32 * 0.7) as i32
            } else {
                // 向上滚动 → 目标偏上(30%)，过冲时元素仍在下半区可见
                r.y + (r.height as f32 * 0.3) as i32
            };
            target
        })
    } else {
        None
    };
    if let Some(vy) = viewport_center_y {
        let dir_label = if delta < 0 { "down→70%" } else { "up→30%" };
        info!("Viewport target Y = {} (direction: {})", vy, dir_label);
    } else if scroll_to_center {
        warn!("scrollToCenter enabled but viewport target Y unavailable");
    }

    let container_height_for_auto = container_rect.as_ref().map(|r| r.height);
    // 容器视口高度，用于百分比计算（threshold、delta 缩放等都基于此）
    let container_height = container_rect.as_ref().map(|r| r.height as f32).unwrap_or(600.0);

    // Step 1.5: 预检查 wait 元素是否已完全可见（在容器视口内且非 offscreen）
    // 如果已可见，直接返回，避免无意义的滚动和鼠标移动
    if let Some(ref wait_xpath) = options.wait {
        if wait_visible {
            let precheck_xpath = wait_xpath.clone();
            let precheck_window = window_selector.clone();
            let precheck_container_rect = container_rect.clone();
            let precheck_result = tokio::task::spawn_blocking(move || {
                super::super::capture::validate_selector_and_xpath_detailed(
                    &precheck_window,
                    &precheck_xpath,
                    &[],
                )
            })
            .await;

            if let Ok(pr) = precheck_result {
                if matches!(pr.overall, ValidationResult::Found { .. }) {
                    let cur_offscreen = pr.is_offscreen.unwrap_or(false);
                    if !cur_offscreen {
                        // 检查元素 rect 是否完全在容器 rect 内
                        let fully_visible = match (&precheck_container_rect, &pr.overall) {
                            (Some(vp), ValidationResult::Found { first_rect: Some(er), .. }) => {
                                let er_api: super::types::Rect = er.clone().into();
                                er_api.x >= vp.x
                                    && er_api.y >= vp.y
                                    && (er_api.x + er_api.width) <= (vp.x + vp.width)
                                    && (er_api.y + er_api.height) <= (vp.y + vp.height)
                            }
                            _ => false,
                        };

                        if fully_visible {
                            info!("Wait element already fully visible, skipping scroll");
                            let target_rect: Option<super::types::Rect> = if let ValidationResult::Found { first_rect, .. } = &pr.overall {
                                first_rect.as_ref().map(|r| r.clone().into())
                            } else {
                                None
                            };
                            return HttpResponse::Ok().json(MouseScrollResponse {
                                success: true,
                                scrolled: 0,
                                target_found: true,
                                target_rect,
                                scrolled_to_end: false,
                                error: None,
                            });
                        }
                    }
                }
            }
        }
    }

    // Step 2: 移动到元素中心
    with_auto_pause(|| async {
        let start_point = mouse_control::get_cursor_position();
        let _ = mouse_control::humanized_move(start_point, scroll_point, 400, "bezier");

        // Step 3: 循环滚动并检测 wait xpath
        let mut scrolled: u32 = 0;
        let start_time = std::time::Instant::now();
        let mut current_delta = delta; // 初始 delta
        let mut did_initial_scroll = false;
        let mut scroll_to_center_adjust_count: u32 = 0; // scrollToCenter 模式下可见后的调整次数
        let original_delta_sign = delta.signum(); // 原始滚动方向符号，调整时始终保持同方向

        // ── 滚动到底检测 ──
        // 每 STUCK_CHECK_INTERVAL 次滚动检测一次鼠标下元素的位置变化
        const STUCK_CHECK_INTERVAL: u32 = 10;
        const STUCK_Y_THRESHOLD: i32 = 2; // 位置变化 <= 2px 视为未移动
        const STUCK_COUNT_THRESHOLD: u32 = 2; // 连续 STUCK_COUNT_THRESHOLD 次检测到未移动则判定到底
        let mut prev_element_y: Option<i32> = None; // 上次检测时鼠标下元素的 Y 坐标
        let mut stuck_count: u32 = 0; // 连续检测到位置未变的次数
        let check_x = scroll_point.x;
        let check_y = scroll_point.y;

        while scrolled < times {
            // 检测 wait xpath
            if let Some(ref wait_xpath) = options.wait {
                if start_time.elapsed().as_millis() as u64 >= timeout_ms {
                    // 超时，返回结果
                    info!("Scroll timeout after {} scrolls", scrolled);
                    return HttpResponse::Ok().json(MouseScrollResponse {
                        success: true,
                        scrolled,
                        target_found: false,
                        target_rect: None,
                        scrolled_to_end: false,
                        error: Some(format!("超时 {}ms", timeout_ms)),
                    });
                }

                // 检测 wait xpath 是否存在
                let wait_xpath_clone = wait_xpath.clone();
                let win_sel = window_selector_for_wait.clone();
                let wait_result = tokio::task::spawn_blocking(move || {
                    super::super::capture::validate_selector_and_xpath_detailed(
                        &win_sel,
                        &wait_xpath_clone,
                        &[],
                    )
                })
                .await;

                if let Ok(wr) = wait_result {
                    if matches!(wr.overall, ValidationResult::Found { .. }) {
                        // 提取目标元素的 rect（用于客户端判断元素是否完全可见）
                        let target_rect: Option<super::types::Rect> = if let ValidationResult::Found { first_rect, .. } = &wr.overall {
                            first_rect.as_ref().map(|r| r.clone().into())
                        } else {
                            None
                        };

                        // 提取当前元素中心 Y（用于测量滚动比例）
                        let cur_element_center_y = if let ValidationResult::Found { first_rect, .. } = &wr.overall {
                            first_rect.as_ref().map(|r| r.y + r.height / 2)
                        } else {
                            None
                        };

                        let cur_offscreen = wr.is_offscreen.unwrap_or(false);

                        if wait_visible {
                            // visible 模式：还需检查元素不在屏幕外
                            if cur_offscreen {
                                // 阶段1：元素在屏幕外，继续同方向滚动
                                info!("Wait xpath found but offscreen, continue scrolling (delta={})", current_delta);
                            } else if !scroll_to_center {
                                // 不需要居中，元素可见即返回
                                info!("Wait xpath found and visible after {} scrolls", scrolled);
                                return HttpResponse::Ok().json(MouseScrollResponse {
                                    success: true,
                                    scrolled,
                                    target_found: true,
                                    target_rect,
                                    scrolled_to_end: false,
                                    error: None,
                                });
                            } else {
                                // 阶段2：元素已可见，scrollToCenter 调整到视口中心
                                scroll_to_center_adjust_count += 1;
                                if scroll_to_center_adjust_count > scroll_to_center_adjust_times {
                                    // 已达最大调整次数，停止滚动避免死循环
                                    info!("Wait xpath visible, scrollToCenter adjust limit reached (adjust_count={}, max={}), stopping after {} scrolls", scroll_to_center_adjust_count, scroll_to_center_adjust_times, scrolled);
                                    return HttpResponse::Ok().json(MouseScrollResponse {
                                        success: true,
                                        scrolled,
                                        target_found: true,
                                        target_rect,
                                        scrolled_to_end: false,
                                        error: Some(format!("scrollToCenter 调整次数已达上限 {}", scroll_to_center_adjust_times)),
                                    });
                                }

                                match (cur_element_center_y, viewport_center_y) {
                                    (Some(ey), Some(vy)) => {
                                        let distance = (ey - vy).abs();
                                        // 居中阈值：视口高度的比例（元素中心距目标中心在此范围内即认为居中）
                                        let threshold = (container_height * options.scroll_to_center_threshold.unwrap_or(0.10)) as i32;
                                        if distance <= threshold {
                                            info!("Wait xpath centered (element_y={}, viewport_y={}, distance={}, threshold={}) after {} scrolls (adjust_count={})", ey, vy, distance, threshold, scrolled, scroll_to_center_adjust_count);
                                            return HttpResponse::Ok().json(MouseScrollResponse {
                                                success: true,
                                                scrolled,
                                                target_found: true,
                                                target_rect,
                                                scrolled_to_end: false,
                                                error: None,
                                            });
                                        } else {
                                            // 同方向渐进缩小策略：
                                            // 始终保持原始滚动方向，只缩小 delta 量级
                                            // 这样不会反向滚动导致过冲，更像人的操作
                                            //
                                            // 距离越大用较大 delta，距离越小用较小 delta
                                            // 使用原始 delta 的绝对值作为基准，按距离占视口高度的比例缩放
                                            let base_abs_delta = delta.abs().max(120);
                                            let distance_ratio = (distance as f32 / container_height).min(1.0);
                                            let scaled = (base_abs_delta as f32 * distance_ratio) as i32;
                                            // 最小 delta 为原始 delta 的 minDeltaRatio 比例，且不低于一个滚轮刻度(120)
                                            let min_delta = (base_abs_delta as f32 * options.min_delta_ratio.unwrap_or(0.1)).max(120.0) as i32;
                                            let adjust_delta = scaled.max(min_delta) * original_delta_sign;
                                            current_delta = adjust_delta;
                                            info!("Same-direction adjust: element_y={}, viewport_y={}, distance={}, threshold={}, distance_ratio={:.2}, base_abs_delta={}, min_delta={}, adjust_delta={} (adjust_count={}/{})",
                                                ey, vy, distance, threshold, distance_ratio, base_abs_delta, min_delta, adjust_delta, scroll_to_center_adjust_count, scroll_to_center_adjust_times);
                                        }
                                    }
                                    _ => {
                                        // 中心数据不可用，继续同方向滚动（不放弃，受 adjust 次数限制）
                                        // 每次缩减到原来的一半，最小为原始 delta 的 minDeltaRatio 比例
                                        let min_delta = (delta.abs() as f32 * options.min_delta_ratio.unwrap_or(0.1)).max(120.0) as i32;
                                        current_delta = (current_delta.abs() / 2).max(min_delta) * original_delta_sign;
                                        info!("Wait xpath visible, scrollToCenter no center data, reduced delta={} (adjust_count={}/{})", current_delta, scroll_to_center_adjust_count, scroll_to_center_adjust_times);
                                    }
                                }
                            }
                        } else {
                            // exist 模式：找到即可
                            info!("Wait xpath found after {} scrolls", scrolled);
                            return HttpResponse::Ok().json(MouseScrollResponse {
                                success: true,
                                scrolled,
                                target_found: true,
                                target_rect,
                                scrolled_to_end: false,
                                error: None,
                            });
                        }
                    }
                }
            }

            // 如果启用了 auto_delta，第一次滚动使用固定 delta，然后计算自适应 delta
            if auto_delta && !did_initial_scroll && scrolled == 0 {
                // 先滚动一次固定 delta
                let _ = mouse_control::scroll_wheel(current_delta);
                did_initial_scroll = true;

                // 等待让页面响应（autoDelta 首次滚动后延迟）
                tokio::time::sleep(tokio::time::Duration::from_millis(options.auto_delta_initial_delay_ms.unwrap_or(1000))).await;

                // 查询容器元素的新 rect 获取可视高度
                if let Some(height) = container_height_for_auto {
                    // 使用容器高度计算自适应 delta，保留原始 delta 的方向（符号）
                    let abs_delta = (height as f32 * delta_factor) as i32;
                    let sign = delta.signum();
                    current_delta = sign * abs_delta;
                    info!("Auto delta calculated: container_height={} factor={} abs_delta={} sign={} final_delta={}", height, delta_factor, abs_delta, sign, current_delta);
                }
                scrolled += 1;
                continue;
            }

            // 执行一次滚动
            let _ = mouse_control::scroll_wheel(current_delta);
            scrolled += 1;

            // 滚动间隔，给页面响应时间
            tokio::time::sleep(tokio::time::Duration::from_millis(options.scroll_interval_ms.unwrap_or(1000))).await;

            // ── 滚动到底检测：每 STUCK_CHECK_INTERVAL 次滚动检测一次鼠标下元素位置 ──
            if scrolled > 0 && scrolled % STUCK_CHECK_INTERVAL == 0 {
                let cx = check_x;
                let cy = check_y;
                let rect_result = tokio::task::spawn_blocking(move || {
                    super::super::core::com_worker::global_get_element_rect_at_point(cx, cy)
                }).await;

                if let Ok(Ok(Some(rect))) = rect_result {
                    let cur_y = rect.y;
                    if let Some(py) = prev_element_y {
                        if (cur_y - py).abs() <= STUCK_Y_THRESHOLD {
                            stuck_count += 1;
                            info!("Scroll stuck detected: prev_y={}, cur_y={}, delta={}, stuck_count={}/{}", py, cur_y, cur_y - py, stuck_count, STUCK_COUNT_THRESHOLD);
                            if stuck_count >= STUCK_COUNT_THRESHOLD {
                                info!("Scroll reached end: element under mouse position unchanged for {} consecutive checks after {} scrolls", stuck_count, scrolled);
                                return HttpResponse::Ok().json(MouseScrollResponse {
                                    success: true,
                                    scrolled,
                                    target_found: false,
                                    target_rect: None,
                                    scrolled_to_end: true,
                                    error: None,
                                });
                            }
                        } else {
                            stuck_count = 0;
                        }
                    }
                    prev_element_y = Some(cur_y);
                } else {
                    // 获取失败，重置追踪状态
                    prev_element_y = None;
                    stuck_count = 0;
                }
            }
        }

        // 全部滚动完成
        info!("Scroll completed {} times", scrolled);
        HttpResponse::Ok().json(MouseScrollResponse {
            success: true,
            scrolled,
            target_found: options.wait.is_none(),
            target_rect: None,
            scrolled_to_end: false,
            error: None,
        })
    }).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动边界检测
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/scroll-detect
/// 滚动一次并检测是否到达边界（内容元素是否不再移动）
///
/// 流程：
/// 1. 获取 container（滚动容器）坐标，移动鼠标到其中心
/// 2. 用 find_visible_elements 在容器内查询指定 ControlType 的可见元素，记录每个元素的 bound.top
/// 3. 执行一次滚动
/// 4. 等待 UI 响应
/// 5. 再次查询容器内可见元素
/// 6. 按 (automationId, name, className) 配对，排除 exclude 列表中的元素
/// 7. 比对：任一非排除元素 bound.top 变化 > 阈值 → 没到底；全部不变 → 到底
/// 8. 若 rollback=true，反向滚动一次恢复位置
pub async fn scroll_detect(body: web::Json<MouseScrollDetectRequest>) -> impl Responder {
    let request = body.into_inner();
    let direction = request.direction.to_lowercase();
    // "down"=向下滚(delta=-120, 检测到底)，"up"=向上滚(delta=120, 检测到顶)
    let delta: i32 = match direction.as_str() {
        "up" => 120,
        "down" => -120,
        _ => -120, // 默认向下
    };
    let rollback = request.rollback;
    let scroll_delay_ms = request.scroll_delay_ms;
    let window_selector = request.window.as_deref().unwrap_or("Window").to_string();
    let control_types = request.control_types.clone();
    let container_xpath = request.container.clone();
    let exclude_xpaths = request.exclude.clone();

    info!(
        "API: /api/mouse/scroll-detect window='{}' container='{}' control_types={:?} direction='{}'(delta={}) exclude={:?} rollback={} scroll_delay_ms={}",
        window_selector, container_xpath, control_types, direction, delta, exclude_xpaths, rollback, scroll_delay_ms
    );

    // ── Helper: 构建错误响应 ──────────────────────────────────────────────
    fn err_resp(msg: &str) -> HttpResponse {
        HttpResponse::Ok().json(MouseScrollDetectResponse {
            success: false,
            at_end: false,
            watched_count: 0,
            changed_count: 0,
            details: vec![],
            rolled_back: false,
            error: Some(msg.to_string()),
        })
    }

    // ── Step 1: 获取 container 坐标 → 移动鼠标 ───────────────────────────
    let container_for_query = container_xpath.clone();
    let window_for_container = window_selector.clone();
    let container_result = tokio::task::spawn_blocking(move || {
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_for_container,
            &container_for_query,
            &[],
        )
    })
    .await;

    use super::super::model::ValidationResult;

    let scroll_point = match container_result {
        Ok(detailed_result) => match &detailed_result.overall {
            ValidationResult::Found { first_rect, .. } => {
                if let Some(rect) = first_rect {
                    let rect_api: super::types::Rect = rect.clone().into();
                    Point::new(
                        rect_api.x + rect_api.width / 2,
                        rect_api.y + rect_api.height / 2,
                    )
                } else {
                    return err_resp("滚动容器坐标获取失败");
                }
            }
            ValidationResult::NotFound => {
                return err_resp(&format!("未找到滚动容器: {}", container_xpath));
            }
            ValidationResult::Error(e) => {
                return err_resp(e);
            }
            _ => {
                return err_resp("滚动容器校验状态未知");
            }
        },
        Err(e) => {
            return HttpResponse::InternalServerError().json(MouseScrollDetectResponse {
                success: false,
                at_end: false,
                watched_count: 0,
                changed_count: 0,
                details: vec![],
                rolled_back: false,
                error: Some(format!("内部错误: {}", e)),
            });
        }
    };

    // ── Step 2: 在自动暂停上下文中执行检测 ───────────────────────────────
    with_auto_pause(|| async {
        // 移动鼠标到滚动容器中心
        let start_point = mouse_control::get_cursor_position();
        let _ = mouse_control::humanized_move(start_point, scroll_point, 400, "bezier");

        // ── 元素快照：用 (automationId, name, className) 作为唯一标识 ──
        #[derive(Clone)]
        struct ElementSnapshot {
            identifier: String,
            top: Option<i32>,
            is_offscreen: bool,
        }

        fn make_identifier(elem: &super::types::ElementInfo) -> String {
            // 优先用 automationId，否则用 name+className 组合
            if !elem.automation_id.is_empty() {
                format!("aid:{}", elem.automation_id)
            } else if !elem.name.is_empty() || !elem.class_name.is_empty() {
                format!("n:{}|c:{}", elem.name, elem.class_name)
            } else {
                // 兜底：用 controlType+rect 位置
                format!("t:{}|y:{:?}", elem.control_type, elem.rect.as_ref().map(|r| r.y))
            }
        }

        fn take_snapshot(
            window_sel: &str,
            container_xp: &str,
            control_types: &[&str],
        ) -> Vec<ElementSnapshot> {
            let elements = crate::core::uia::find_visible_elements(
                window_sel,
                container_xp,
                control_types,
            );
            elements.iter().map(|elem| {
                let identifier = make_identifier(elem);
                let top = elem.rect.as_ref().map(|r| r.y);
                ElementSnapshot {
                    identifier,
                    top,
                    is_offscreen: elem.is_offscreen, // find_visible_elements 只返回可见元素，此值恒为 false
                }
            }).collect()
        }

        // ── Step 3: 滚动前快照 ──────────────────────────────────────────
        let ws_before = window_selector.clone();
        let ct_before = container_xpath.clone();
        let ct_refs_before = control_types.clone();
        let before_snapshots = tokio::task::spawn_blocking(move || {
            let ct_refs: Vec<&str> = ct_refs_before.iter().map(|s| s.as_str()).collect();
            take_snapshot(&ws_before, &ct_before, &ct_refs)
        }).await.unwrap_or_default();

        info!("Scroll detect: before snapshot {} visible elements", before_snapshots.len());

        // ── Step 4: 查询 exclude 元素的标识集合 ─────────────────────────
        let exclude_identifiers: std::collections::HashSet<String> = {
            let mut set = std::collections::HashSet::new();
            for ex_xpath in &exclude_xpaths {
                let ws = window_selector.clone();
                let xp = ex_xpath.clone();
                let ex_elements = tokio::task::spawn_blocking(move || {
                    crate::core::uia::find_all_elements_detailed(&ws, &xp, 0.0)
                }).await.unwrap_or_default();
                for elem in &ex_elements {
                    set.insert(make_identifier(elem));
                }
            }
            set
        };

        // ── Step 5: 执行滚动 ─────────────────────────────────────────────
        let _ = mouse_control::scroll_wheel(delta);

        // ── Step 6: 等待 UI 响应（滚动动画 + UIA 属性刷新） ─────────────
        tokio::time::sleep(tokio::time::Duration::from_millis(scroll_delay_ms)).await;

        // ── Step 7: 滚动后快照 ──────────────────────────────────────────
        let ws_after = window_selector.clone();
        let ct_after = container_xpath.clone();
        let ct_refs_after = control_types.clone();
        let after_snapshots = tokio::task::spawn_blocking(move || {
            let ct_refs: Vec<&str> = ct_refs_after.iter().map(|s| s.as_str()).collect();
            take_snapshot(&ws_after, &ct_after, &ct_refs)
        }).await.unwrap_or_default();

        info!("Scroll detect: after snapshot {} visible elements", after_snapshots.len());

        // ── Step 8: 前后配对比对 ─────────────────────────────────────────
        // 用 identifier 建立 after 的查找表
        let after_map: std::collections::HashMap<String, &ElementSnapshot> = after_snapshots
            .iter()
            .map(|s| (s.identifier.clone(), s))
            .collect();

        // 同时构建 before 的 identifier 集合，用于检测"新出现"的元素
        let before_identifiers: std::collections::HashSet<String> = before_snapshots
            .iter()
            .map(|s| s.identifier.clone())
            .collect();

        const TOP_THRESHOLD: i32 = 2; // bound.top 变化阈值
        let mut watched_count: usize = 0;
        let mut changed_count: usize = 0;
        let mut details: Vec<super::types::ElementChangeDetail> = Vec::new();

        for before in &before_snapshots {
            // 排除 exclude 列表中的元素
            if exclude_identifiers.contains(&before.identifier) {
                continue;
            }

            watched_count += 1;

            if let Some(after) = after_map.get(&before.identifier) {
                // 配对成功：比较 bound.top 变化
                let top_changed = match (before.top, after.top) {
                    (Some(b), Some(a)) => (a - b).abs() > TOP_THRESHOLD,
                    _ => false,
                };
                let offscreen_changed = before.is_offscreen != after.is_offscreen;
                let changed = top_changed || offscreen_changed;

                if changed {
                    changed_count += 1;
                    let delta_top = match (before.top, after.top) {
                        (Some(b), Some(a)) => Some(a - b),
                        _ => None,
                    };
                    details.push(super::types::ElementChangeDetail {
                        identifier: before.identifier.clone(),
                        before_top: before.top,
                        after_top: after.top,
                        delta_top,
                        offscreen_changed,
                    });
                }
            } else {
                // 元素在 before 可见但 after 中消失 → 说明滚动了
                // （滚出可视区域，或在虚拟化列表中被回收）
                changed_count += 1;
                details.push(super::types::ElementChangeDetail {
                    identifier: before.identifier.clone(),
                    before_top: before.top,
                    after_top: None,
                    delta_top: None,
                    offscreen_changed: true, // 从可见变为不可见
                });
            }
        }

        // 检查 after 中新出现的元素（在 before 中不存在）
        // 新元素出现 = 内容滚入 = 没到底
        for after in &after_snapshots {
            if exclude_identifiers.contains(&after.identifier) {
                continue;
            }
            if !before_identifiers.contains(&after.identifier) {
                watched_count += 1;
                changed_count += 1;
                details.push(super::types::ElementChangeDetail {
                    identifier: after.identifier.clone(),
                    before_top: None,
                    after_top: after.top,
                    delta_top: None,
                    offscreen_changed: true, // 从不可见变为可见
                });
            }
        }

        // ── Step 9: 判定 atEnd ───────────────────────────────────────────
        // 任一变化 → 没到底；全部不变 → 到底
        let at_end = watched_count > 0 && changed_count == 0;

        info!(
            "Scroll detect: watched={} changed={} at_end={}",
            watched_count, changed_count, at_end
        );

        // ── Step 10: 若 rollback=true，反向滚动恢复位置 ──────────────────
        let mut rolled_back = false;
        if rollback {
            let rollback_delta = -delta; // 反方向
            let _ = mouse_control::scroll_wheel(rollback_delta);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            rolled_back = true;
            info!("Scroll detect rollback: direction={}(delta={})", if delta > 0 { "down" } else { "up" }, rollback_delta);
        }

        HttpResponse::Ok().json(MouseScrollDetectResponse {
            success: true,
            at_end,
            watched_count,
            changed_count,
            details,
            rolled_back,
            error: None,
        })
    })
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// 悬停 & 拖拽
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/hover
/// 鼠标悬停在元素上（触发 tooltip/hover 菜单）
pub async fn hover_mouse(body: web::Json<MouseHoverRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseHoverOptions::default());

    info!(
        "API: /api/mouse/hover window='{}' element='{}' humanize={} duration={}ms",
        match &request.window {
            super::types::WindowSelectorOrString::String(s) => s.as_str(),
            super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or(""),
        },
        request.element,
        options.humanize,
        options.duration
    );

    let window_selector = build_window_selector(&request.window);
    let element = request.element.clone();

    let element_result = tokio::task::spawn_blocking(move || {
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector,
            &element,
            &[],
        )
    })
    .await;

    match element_result {
        Ok(detailed_result) => {
            use super::super::model::ValidationResult;
            match &detailed_result.overall {
                ValidationResult::Found { first_rect, .. } => {
                    if let Some(rect) = first_rect {
                        let rect_api: super::types::Rect = rect.clone().into();
                        let hover_point = rect_api.center();

                        with_auto_pause(|| async {
                            // 确保窗口在前台
                            let _ = super::super::core::uia::activate_window_by_selector(
                                &build_window_selector(&request.window),
                            );
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                            if options.humanize {
                                let start = mouse_control::get_cursor_position();
                                let _ = mouse_control::humanized_move(
                                    start, hover_point, options.duration / 2, "bezier",
                                );
                            } else {
                                let _ = mouse_control::linear_move(
                                    mouse_control::get_cursor_position(), hover_point,
                                );
                            }

                            // 悬停停留
                            tokio::time::sleep(tokio::time::Duration::from_millis(options.duration)).await;

                            info!("Hover completed at ({}, {})", hover_point.x, hover_point.y);
                            HttpResponse::Ok().json(MouseHoverResponse {
                                success: true,
                                hover_point,
                                error: None,
                            })
                        }).await
                    } else {
                        HttpResponse::Ok().json(MouseHoverResponse {
                            success: false,
                            hover_point: Point::new(0, 0),
                            error: Some("元素坐标获取失败".to_string()),
                        })
                    }
                }
                ValidationResult::NotFound => {
                    HttpResponse::Ok().json(MouseHoverResponse {
                        success: false,
                        hover_point: Point::new(0, 0),
                        error: Some(format!("未找到匹配元素")),
                    })
                }
                ValidationResult::Error(e) => {
                    HttpResponse::Ok().json(MouseHoverResponse {
                        success: false,
                        hover_point: Point::new(0, 0),
                        error: Some(e.clone()),
                    })
                }
                _ => {
                    HttpResponse::Ok().json(MouseHoverResponse {
                        success: false,
                        hover_point: Point::new(0, 0),
                        error: Some("校验状态未知".to_string()),
                    })
                }
            }
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(MouseHoverResponse {
                success: false,
                hover_point: Point::new(0, 0),
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// POST /api/mouse/drag
/// 从源元素拖拽到目标元素
pub async fn drag_mouse(body: web::Json<MouseDragRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseDragOptions::default());

    info!(
        "API: /api/mouse/drag window='{}' source='{}' target='{}' duration={}ms",
        match &request.window {
            super::types::WindowSelectorOrString::String(s) => s.as_str(),
            super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or(""),
        },
        request.source_element,
        request.target_element,
        options.duration
    );

    let window_selector = build_window_selector(&request.window);
    let source_xpath = request.source_element.clone();
    let target_xpath = request.target_element.clone();

    // 查询源元素和目标元素坐标
    let element_result = tokio::task::spawn_blocking(move || {
        let source_result = super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector, &source_xpath, &[],
        );
        let target_result = super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector, &target_xpath, &[],
        );
        (source_result, target_result)
    })
    .await;

    let (source_result, target_result) = match element_result {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError().json(MouseDragResponse {
                success: false,
                source_point: Point::new(0, 0),
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(format!("内部错误: {}", e)),
            });
        }
    };

    use super::super::model::ValidationResult;

    let source_point = match &source_result.overall {
        ValidationResult::Found { first_rect, .. } => {
            if let Some(rect) = first_rect {
                let rect_api: super::types::Rect = rect.clone().into();
                rect_api.center()
            } else {
                return HttpResponse::Ok().json(MouseDragResponse {
                    success: false,
                    source_point: Point::new(0, 0),
                    target_point: Point::new(0, 0),
                    duration_ms: 0,
                    error: Some("源元素坐标获取失败".to_string()),
                });
            }
        }
        ValidationResult::NotFound => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point: Point::new(0, 0),
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(format!("未找到源元素: {}", request.source_element)),
            });
        }
        ValidationResult::Error(e) => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point: Point::new(0, 0),
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(e.clone()),
            });
        }
        _ => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point: Point::new(0, 0),
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some("校验状态未知".to_string()),
            });
        }
    };

    let target_point = match &target_result.overall {
        ValidationResult::Found { first_rect, .. } => {
            if let Some(rect) = first_rect {
                let rect_api: super::types::Rect = rect.clone().into();
                rect_api.center()
            } else {
                return HttpResponse::Ok().json(MouseDragResponse {
                    success: false,
                    source_point,
                    target_point: Point::new(0, 0),
                    duration_ms: 0,
                    error: Some("目标元素坐标获取失败".to_string()),
                });
            }
        }
        ValidationResult::NotFound => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point,
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(format!("未找到目标元素: {}", request.target_element)),
            });
        }
        ValidationResult::Error(e) => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point,
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(e.clone()),
            });
        }
        _ => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point,
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some("校验状态未知".to_string()),
            });
        }
    };

    with_auto_pause(|| async {
        // 确保窗口在前台
        let _ = super::super::core::uia::activate_window_by_selector(
            &build_window_selector(&request.window),
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let drag_start = std::time::Instant::now();
        let result = mouse_control::drag_mouse(source_point, target_point, options.duration);
        let drag_duration = drag_start.elapsed().as_millis() as u64;

        match result {
            Ok(_) => {
                info!("Drag completed: ({}, {}) -> ({}, {})",
                    source_point.x, source_point.y, target_point.x, target_point.y);
                HttpResponse::Ok().json(MouseDragResponse {
                    success: true,
                    source_point,
                    target_point,
                    duration_ms: drag_duration,
                    error: None,
                })
            }
            Err(e) => {
                warn!("Drag failed: {}", e);
                HttpResponse::Ok().json(MouseDragResponse {
                    success: false,
                    source_point,
                    target_point,
                    duration_ms: 0,
                    error: Some(format!("拖拽失败: {}", e)),
                })
            }
        }
    }).await
}
