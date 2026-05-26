// src/api/mouse.rs
//
// 鼠标操作 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};

use super::types::{
    MouseMoveRequest, MouseMoveResponse, MouseMoveOptions,
    MouseClickRequest, MouseClickResponse, MouseClickOptions,
    MouseScrollRequest, MouseScrollResponse, MouseScrollOptions,
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
        if let Err(e) = super::super::core::uia::windows_impl::ensure_com_sta() {
            log::error!("COM STA init failed: {}", e);
        }

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

    info!(
        "API: /api/mouse/scroll element='{}' times={} delta={} auto_delta={} delta_factor={} wait={:?} timeout={}ms",
        request.element, times, delta, auto_delta, delta_factor, options.wait, timeout_ms
    );

    // Step 1: 获取元素坐标
    let element_for_query = request.element.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        if let Err(e) = super::super::core::uia::windows_impl::ensure_com_sta() {
            log::error!("COM STA init failed: {}", e);
        }

        super::super::capture::validate_selector_and_xpath_detailed(
            "Window",
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
                error: Some(format!("内部错误: {}", e)),
            });
        }
    };

    use super::super::model::ValidationResult;

    let (scroll_point, container_height_for_auto) = match &element_result.overall {
        ValidationResult::Found { first_rect, .. } => {
            if let Some(rect) = first_rect {
                let rect_api: super::types::Rect = rect.clone().into();
                let point = Point::new(
                    rect_api.x as i32 + rect_api.width as i32 / 2,
                    rect_api.y as i32 + rect_api.height as i32 / 2,
                );
                (point, Some(rect_api.height))
            } else {
                return HttpResponse::Ok().json(MouseScrollResponse {
                    success: false,
                    scrolled: 0,
                    target_found: false,
                    error: Some("元素坐标获取失败".to_string()),
                });
            }
        }
        ValidationResult::NotFound => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                error: Some(format!("未找到元素: {}", request.element)),
            });
        }
        ValidationResult::Error(e) => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                error: Some(e.clone()),
            });
        }
        _ => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                error: Some("校验状态未知".to_string()),
            });
        }
    };

    // Step 2: 移动到元素中心
    with_auto_pause(|| async {
        let start_point = mouse_control::get_cursor_position();
        let _ = mouse_control::humanized_move(start_point, scroll_point, 400, "bezier");

        // Step 3: 循环滚动并检测 wait xpath
        let mut scrolled: u32 = 0;
        let start_time = std::time::Instant::now();
        let mut current_delta = delta; // 初始 delta
        let mut did_initial_scroll = false;

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
                        error: Some(format!("超时 {}ms", timeout_ms)),
                    });
                }

                // 检测 wait xpath 是否存在
                let wait_xpath_clone = wait_xpath.clone();
                let wait_result = tokio::task::spawn_blocking(move || {
                    if let Err(e) = super::super::core::uia::windows_impl::ensure_com_sta() {
                        log::error!("COM STA init failed: {}", e);
                    }
                    super::super::capture::validate_selector_and_xpath_detailed(
                        "Window",
                        &wait_xpath_clone,
                        &[],
                    )
                })
                .await;

                if let Ok(wr) = wait_result {
                    if matches!(wr.overall, ValidationResult::Found { .. }) {
                        // 找到目标，返回
                        info!("Wait xpath found after {} scrolls", scrolled);
                        return HttpResponse::Ok().json(MouseScrollResponse {
                            success: true,
                            scrolled,
                            target_found: true,
                            error: None,
                        });
                    }
                }
            }

            // 如果启用了 auto_delta，第一次滚动使用固定 delta，然后计算自适应 delta
            if auto_delta && !did_initial_scroll && scrolled == 0 {
                // 先滚动一次固定 delta
                let _ = mouse_control::scroll_wheel(current_delta);
                did_initial_scroll = true;

                // 短暂等待让页面响应
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                // 查询容器元素的新 rect 获取可视高度
                if let Some(height) = container_height_for_auto {
                    // 使用容器高度计算自适应 delta
                    current_delta = (height as f32 * delta_factor) as i32;
                    info!("Auto delta calculated: container_height={} factor={} delta={}", height, delta_factor, current_delta);
                }
                scrolled += 1;
                continue;
            }

            // 执行一次滚动
            let _ = mouse_control::scroll_wheel(current_delta);
            scrolled += 1;

            // 滚动间隔，给页面响应时间
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        }

        // 全部滚动完成
        info!("Scroll completed {} times", scrolled);
        HttpResponse::Ok().json(MouseScrollResponse {
            success: true,
            scrolled,
            target_found: options.wait.is_none(),
            error: None,
        })
    }).await
}
