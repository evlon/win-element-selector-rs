// src/api/element.rs
//
// 元素查找 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::types::{ElementQuery, ElementResponse, ElementInfo};

// ═══════════════════════════════════════════════════════════════════════════════
// 多元素查找响应类型
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
pub struct AllElementsResponse {
    /// 是否找到元素
    pub found: bool,
    /// 元素列表
    pub elements: Vec<ElementInfo>,
    /// 总数量
    pub total: usize,
    /// 错误信息
    pub error: Option<String>,
}

/// GET/POST /api/element
/// 根据窗口选择器和 XPath 获取元素信息及坐标
pub async fn get_element(
    query: Option<web::Query<ElementQuery>>,
    body: Option<web::Json<ElementQuery>>,
) -> impl Responder {
    // 支持 GET 和 POST 两种方式
    let element_query = if let Some(q) = query {
        q.into_inner()
    } else if let Some(b) = body {
        b.into_inner()
    } else {
        return HttpResponse::BadRequest().json(ElementResponse {
            found: false,
            element: None,
            error: Some("缺少查询参数".to_string()),
        });
    };
    
    info!(
        "API: /api/element window_selector='{}' xpath='{}' random_range={}",
        element_query.window_selector, element_query.xpath, element_query.random_range
    );
    
    // Clone query for spawn_blocking (需要 'static)
    let window_selector = element_query.window_selector.clone();
    let xpath = element_query.xpath.clone();
    let random_range = element_query.random_range;
    
    // Use global COM worker thread (single-threaded COM management)
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(window_selector, xpath, Some(random_range))
    })
    .await;
    
    match result {
        Ok(Ok(elements)) => {
            if let Some(element_info) = elements.into_iter().next() {
                info!(
                    "Element found: type='{}' name='{}' center=({},{})",
                    element_info.control_type, element_info.name,
                    element_info.center.x, element_info.center.y
                );
                HttpResponse::Ok().json(ElementResponse {
                    found: true,
                    element: Some(element_info),
                    error: None,
                })
            } else {
                warn!("Element not found");
                HttpResponse::Ok().json(ElementResponse {
                    found: false,
                    element: None,
                    error: Some("未找到匹配元素".to_string()),
                })
            }
        }
        Ok(Err(e)) => {
            warn!("COM worker error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element: None,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

/// GET/POST /api/element/all
/// 根据窗口选择器和 XPath 获取所有匹配元素
pub async fn get_all_elements(
    query: Option<web::Query<ElementQuery>>,
    body: Option<web::Json<ElementQuery>>,
) -> impl Responder {
    // 支持 GET 和 POST 两种方式
    let element_query = if let Some(q) = query {
        q.into_inner()
    } else if let Some(b) = body {
        b.into_inner()
    } else {
        return HttpResponse::BadRequest().json(AllElementsResponse {
            found: false,
            elements: vec![],
            total: 0,
            error: Some("缺少查询参数".to_string()),
        });
    };
    
    info!(
        "API: /api/element/all window_selector='{}' xpath='{}' random_range={}",
        element_query.window_selector, element_query.xpath, element_query.random_range
    );
    
    let window_selector = element_query.window_selector.clone();
    let xpath = element_query.xpath.clone();
    let random_range = element_query.random_range;
    
    // Use global COM worker thread
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(window_selector, xpath, Some(random_range))
    })
    .await;
    
    match result {
        Ok(Ok(elements)) => {
            let total = elements.len();
            if total > 0 {
                info!("Found {} elements", total);
                HttpResponse::Ok().json(AllElementsResponse {
                    found: true,
                    elements,
                    total,
                    error: None,
                })
            } else {
                warn!("No elements found");
                HttpResponse::Ok().json(AllElementsResponse {
                    found: false,
                    elements: vec![],
                    total: 0,
                    error: Some("未找到匹配元素".to_string()),
                })
            }
        }
        Ok(Err(e)) => {
            warn!("COM worker error: {}", e);
            HttpResponse::InternalServerError().json(AllElementsResponse {
                found: false,
                elements: vec![],
                total: 0,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(AllElementsResponse {
                found: false,
                elements: vec![],
                total: 0,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}