// src/api/mouse.rs
//
// 鼠标操作 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};

use super::types::{
    MouseMoveRequest, MouseMoveResponse, MouseMoveOptions,
    MouseClickRequest, MouseClickResponse, MouseClickOptions,
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
        "API: /api/mouse/click window.title='{}' xpath='{}' humanize={} random_range={}",
        request.window.title.as_deref().unwrap_or(""),
        request.xpath,
        options.humanize,
        options.random_range
    );
    
    // Step 1: 构建窗口选择器
    let window_selector = build_window_selector(&request.window);
    
    // Step 2: 获取元素坐标
    let xpath = request.xpath.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        if let Err(e) = super::super::core::uia::windows_impl::ensure_com_sta() {
            log::error!("COM STA init failed: {}", e);
        }
        
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector,
            &xpath,
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
                        let click_point = calculate_random_click_point(&rect_api, options.random_range);
                        
                        // Step 3: 使用 with_auto_pause 执行拟人化移动和点击
                        let click_point_copy = click_point;
                        let options_copy = options.clone();
                        
                        with_auto_pause(|| async {
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
                            let click_result = mouse_control::click_at(click_point_copy);
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
fn build_window_selector(selector: &super::types::WindowSelector) -> String {
    let mut predicates: Vec<String> = Vec::new();
    
    if let Some(ref title) = selector.title {
        predicates.push(format!("@Name='{}'", title));
    }
    if let Some(ref class_name) = selector.class_name {
        predicates.push(format!("@ClassName='{}'", class_name));
    }
    if let Some(ref process_name) = selector.process_name {
        predicates.push(format!("@ProcessName='{}'", process_name));
    }
    
    if predicates.is_empty() {
        "Window".to_string()
    } else {
        format!("Window[{}]", predicates.join(" and "))
    }
}

/// 计算随机点击点
fn calculate_random_click_point(rect: &super::types::Rect, range_percent: f32) -> Point {
    use rand::Rng;
    
    let center = rect.center();
    let half_range_w = rect.width as f32 * range_percent / 2.0;
    let half_range_h = rect.height as f32 * range_percent / 2.0;
    
    let mut rng = rand::thread_rng();
    let offset_x = rng.gen_range(-half_range_w..half_range_w) as i32;
    let offset_y = rng.gen_range(-half_range_h..half_range_h) as i32;
    
    Point::new(center.x + offset_x, center.y + offset_y)
}