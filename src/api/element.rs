// src/api/element.rs
//
// 元素查找 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::types::{ElementQuery, ElementResponse};

// ═══════════════════════════════════════════════════════════════════════════════
// 多元素查找响应类型
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素信息附带其选择器
#[derive(Debug, Serialize, Deserialize)]
pub struct ElementWithSelector {
    #[serde(rename = "elementSelector")]
    pub element_selector: String,
    pub info: super::types::ElementInfo,
}

/// 多元素查找响应
#[derive(Debug, Serialize, Deserialize)]
pub struct AllElementsResponse {
    pub found: bool,
    pub elements: Vec<ElementWithSelector>,
    pub total: usize,
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
            element_selector: String::new(),
            element: None,
            total: 0,
            error: Some("缺少查询参数".to_string()),
        });
    };

    info!(
        "API: /api/element window='{}' element='{}' random_range={}",
        element_query.window, element_query.element, element_query.random_range
    );

    // Clone query for spawn_blocking (需要 'static)
    let window = element_query.window.clone();
    let element = element_query.element.clone();
    let random_range = element_query.random_range;

    // Use global COM worker thread (single-threaded COM management)
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(window, element, Some(random_range))
    })
    .await;

    match result {
        Ok(Ok(elements)) => {
            let total = elements.len();
            if let Some(element_info) = elements.into_iter().next() {
                let center_pos = element_info.center
                    .map(|c| format!("({}, {})", c.x, c.y))
                    .unwrap_or_else(|| "N/A".to_string());
                info!(
                    "Element found: type='{}' name='{}' center={} (total={})",
                    element_info.control_type, element_info.name, center_pos, total
                );
                HttpResponse::Ok().json(ElementResponse {
                    found: true,
                    element_selector: element_query.element.clone(),
                    element: Some(element_info),
                    total,
                    error: None,
                })
            } else {
                warn!("Element not found");
                HttpResponse::Ok().json(ElementResponse {
                    found: false,
                    element_selector: element_query.element.clone(),
                    element: None,
                    total: 0,
                    error: Some("未找到匹配元素".to_string()),
                })
            }
        }
        Ok(Err(e)) => {
            warn!("COM worker error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element_selector: element_query.element.clone(),
                element: None,
                total: 0,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element_selector: element_query.element.clone(),
                element: None,
                total: 0,
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
        "API: /api/element/all window='{}' element='{}' random_range={}",
        element_query.window, element_query.element, element_query.random_range
    );

    let window = element_query.window.clone();
    let element = element_query.element.clone();
    let random_range = element_query.random_range;

    // Use global COM worker thread
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(window, element, Some(random_range))
    })
    .await;

    match result {
        Ok(Ok(elements)) => {
            let total = elements.len();
            if total > 0 {
                info!("Found {} elements", total);
                let elements_with_selector: Vec<ElementWithSelector> = elements
                    .into_iter()
                    .map(|el_info| ElementWithSelector {
                        element_selector: element_query.element.clone(),
                        info: el_info,
                    })
                    .collect();
                HttpResponse::Ok().json(AllElementsResponse {
                    found: true,
                    elements: elements_with_selector,
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
