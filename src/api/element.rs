// src/api/element.rs
//
// 元素查找 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};

use super::types::{ElementQuery, ElementResponse, ElementInfo, Rect, Point};
use super::super::model::ValidationResult;

/// GET /api/element
/// 根据窗口选择器和 XPath 获取元素信息及坐标
pub async fn get_element(query: web::Query<ElementQuery>) -> impl Responder {
    info!(
        "API: /api/element window_selector='{}' xpath='{}' random_range={}",
        query.window_selector, query.xpath, query.random_range
    );
    
    // Clone query for spawn_blocking (需要 'static)
    let window_selector = query.window_selector.clone();
    let xpath = query.xpath.clone();
    
    // UI Automation 操作需要在 STA 线程中执行
    let result = tokio::task::spawn_blocking(move || {
        // 在阻塞线程中初始化 COM (STA) - UI Automation 需要
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        }
        
        super::super::capture::validate_selector_and_xpath_detailed(
            &window_selector,
            &xpath,
        )
    })
    .await;
    
    match result {
        Ok(detailed_result) => {
            match &detailed_result.overall {
                ValidationResult::Found { count, first_rect } => {
                    if let Some(rect) = first_rect {
                        let api_rect: Rect = rect.clone().into();
                        let center = api_rect.center();
                        let center_random = calculate_random_center(&api_rect, query.random_range);
                        
                        // 获取元素名称和类型（需要额外查询）
                        let element_info = ElementInfo {
                            rect: api_rect,
                            center,
                            center_random,
                            control_type: "Element".to_string(), // 可从 XPath 推断
                            name: String::new(),
                            is_enabled: true,
                        };
                        
                        info!("Element found: {} matches, center=({}, {})", count, center.x, center.y);
                        
                        HttpResponse::Ok().json(ElementResponse {
                            found: true,
                            element: Some(element_info),
                            error: None,
                        })
                    } else {
                        warn!("Element found but no rect available");
                        HttpResponse::Ok().json(ElementResponse {
                            found: true,
                            element: None,
                            error: Some("元素坐标获取失败".to_string()),
                        })
                    }
                }
                ValidationResult::NotFound => {
                    warn!("Element not found");
                    HttpResponse::Ok().json(ElementResponse {
                        found: false,
                        element: None,
                        error: Some(format!(
                            "未找到匹配元素 (耗时 {}ms)",
                            detailed_result.total_duration_ms
                        )),
                    })
                }
                ValidationResult::Error(e) => {
                    warn!("Validation error: {}", e);
                    HttpResponse::Ok().json(ElementResponse {
                        found: false,
                        element: None,
                        error: Some(e.clone()),
                    })
                }
                _ => {
                    HttpResponse::Ok().json(ElementResponse {
                        found: false,
                        element: None,
                        error: Some("校验状态未知".to_string()),
                    })
                }
            }
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// 计算随机中心点（在指定百分比范围内随机）
fn calculate_random_center(rect: &Rect, range_percent: f32) -> Point {
    use rand::Rng;
    
    let center = rect.center();
    let half_range_w = rect.width as f32 * range_percent / 2.0;
    let half_range_h = rect.height as f32 * range_percent / 2.0;
    
    let mut rng = rand::thread_rng();
    let offset_x = rng.gen_range(-half_range_w..half_range_w) as i32;
    let offset_y = rng.gen_range(-half_range_h..half_range_h) as i32;
    
    Point::new(center.x + offset_x, center.y + offset_y)
}