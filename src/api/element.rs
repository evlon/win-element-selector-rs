// src/api/element.rs
//
// 元素查找 API

use std::time::Instant;

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::core::metrics::{next_request_id, selector_hash, xpath_meta};
use super::types::{ElementQuery, ElementResponse, ElementVisibilityRequest, ElementVisibilityResponse, ElementFlashRequest, ElementFlashResponse, Rect, InspectRequest, InspectResponse, InspectNodeInfo, FlatInspectNodeInfo, NavigateRequest, NavigateResponse};

// ═══════════════════════════════════════════════════════════════════════════════
// 多元素查找响应类型
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素信息附带其选择器
#[derive(Debug, Serialize, Deserialize)]
pub struct ElementWithSelector {
    #[serde(rename = "findSelector")]
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

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&element_query.window);
    let element_meta = xpath_meta(&element_query.element);

    info!(
        "[PERF][HTTP][{}] /api/element start window_hash={:016x} {} random_range={}",
        request_id, window_hash, element_meta, element_query.random_range
    );

    // Clone query for spawn_blocking (需要 'static)
    let window = element_query.window.clone();
    let element = element_query.element.clone();
    let random_range = element_query.random_range;

    // Use global COM worker thread (single-threaded COM management)
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(request_id, window, element, Some(random_range))
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
                info!(
                    "[PERF][HTTP][{}] /api/element done status=ok found=true total={} duration_ms={}",
                    request_id,
                    total,
                    request_start.elapsed().as_millis()
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
                info!(
                    "[PERF][HTTP][{}] /api/element done status=ok found=false total=0 duration_ms={}",
                    request_id,
                    request_start.elapsed().as_millis()
                );
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
            warn!(
                "[PERF][HTTP][{}] /api/element done status=com_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
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
            warn!(
                "[PERF][HTTP][{}] /api/element done status=spawn_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
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

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&element_query.window);
    let element_meta = xpath_meta(&element_query.element);

    info!(
        "[PERF][HTTP][{}] /api/element/all start window_hash={:016x} {} random_range={}",
        request_id, window_hash, element_meta, element_query.random_range
    );

    let window = element_query.window.clone();
    let element = element_query.element.clone();
    let random_range = element_query.random_range;

    // Use global COM worker thread
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(request_id, window, element, Some(random_range))
    })
    .await;

    match result {
        Ok(Ok(elements)) => {
            let total = elements.len();
            if total > 0 {
                info!("Found {} elements", total);
                info!(
                    "[PERF][HTTP][{}] /api/element/all done status=ok found=true total={} duration_ms={}",
                    request_id,
                    total,
                    request_start.elapsed().as_millis()
                );
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
                info!(
                    "[PERF][HTTP][{}] /api/element/all done status=ok found=false total=0 duration_ms={}",
                    request_id,
                    request_start.elapsed().as_millis()
                );
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
            warn!(
                "[PERF][HTTP][{}] /api/element/all done status=com_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(AllElementsResponse {
                found: false,
                elements: vec![],
                total: 0,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/all done status=spawn_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(AllElementsResponse {
                found: false,
                elements: vec![],
                total: 0,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

/// POST /api/element/visibility
/// 获取元素在可视区域的位置信息
pub async fn get_element_visibility(body: web::Json<ElementVisibilityRequest>) -> impl Responder {
    let request = body.into_inner();

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&request.window);
    let element_meta = xpath_meta(&request.element);

    info!(
        "[PERF][HTTP][{}] /api/element/visibility start window_hash={:016x} {} container={}",
        request_id,
        window_hash,
        element_meta,
        request.container.as_ref().map_or("none".to_string(), |v| format!("hash={:016x}", selector_hash(v)))
    );

    let window = request.window.clone();
    let element = request.element.clone();
    let container = request.container.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_get_element_visibility(request_id, window, element, container)
    })
    .await;

    match result {
        Ok(Ok(response)) => {
            info!(
                "Element visibility: found={} visibility={} position={} scroll_direction={:?}",
                response.found, response.visibility, response.position, response.scroll_direction
            );
            info!(
                "[PERF][HTTP][{}] /api/element/visibility done status=ok found={} visibility={} duration_ms={}",
                request_id,
                response.found,
                response.visibility,
                request_start.elapsed().as_millis()
            );
            HttpResponse::Ok().json(response)
        }
        Ok(Err(e)) => {
            warn!("Element visibility check failed: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/visibility done status=com_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::Ok().json(ElementVisibilityResponse {
                found: false,
                is_offscreen: None,
                visibility: "error".to_string(),
                position: "unknown".to_string(),
                element_rect: None,
                visible_rect: None,
                viewport_rect: None,
                overflow: None,
                scroll_direction: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Element visibility spawn error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/visibility done status=spawn_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(ElementVisibilityResponse {
                found: false,
                is_offscreen: None,
                visibility: "error".to_string(),
                position: "unknown".to_string(),
                element_rect: None,
                visible_rect: None,
                viewport_rect: None,
                overflow: None,
                scroll_direction: None,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

/// POST /api/element/flash
/// 在元素位置显示高亮闪烁，指定时间后自动消失
pub async fn flash_element(body: web::Json<ElementFlashRequest>) -> impl Responder {
    let request = body.into_inner();

    info!(
        "API: /api/element/flash window='{}' element='{}' timeout={}ms",
        request.window, request.element, request.timeout
    );

    let window = request.window.clone();
    let element = request.element.clone();
    let timeout = request.timeout;

    // 查找元素获取其矩形区域
    let request_id = next_request_id();
    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_find_element(request_id, window, element, None)
    })
    .await;

    match result {
        Ok(Ok(elements)) => {
            if let Some(element_info) = elements.into_iter().next() {
                if let Some(rect) = &element_info.rect {
                    let model_rect = crate::core::model::ElementRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    };
                    let info = crate::core::model::HighlightInfo::new(
                        model_rect,
                        &element_info.control_type,
                    );
                    // 在独立线程中执行高亮闪烁
                    crate::highlight::flash_with_info(&info, timeout);

                    let api_rect: Rect = crate::core::model::ElementRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    }.into();

                    HttpResponse::Ok().json(ElementFlashResponse {
                        success: true,
                        element_rect: Some(api_rect),
                        error: None,
                    })
                } else {
                    HttpResponse::Ok().json(ElementFlashResponse {
                        success: false,
                        element_rect: None,
                        error: Some("元素无矩形区域信息".to_string()),
                    })
                }
            } else {
                HttpResponse::Ok().json(ElementFlashResponse {
                    success: false,
                    element_rect: None,
                    error: Some("未找到匹配元素".to_string()),
                })
            }
        }
        Ok(Err(e)) => {
            warn!("Flash element COM worker error: {}", e);
            HttpResponse::InternalServerError().json(ElementFlashResponse {
                success: false,
                element_rect: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Flash element spawn error: {}", e);
            HttpResponse::InternalServerError().json(ElementFlashResponse {
                success: false,
                element_rect: None,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

/// POST /api/element/inspect
/// 遍历指定元素下的所有子元素，提取层级/控件类型/name/Text/rect/相对xpath
pub async fn inspect_element(body: web::Json<InspectRequest>) -> impl Responder {
    let request = body.into_inner();

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&request.window);
    let element_meta = xpath_meta(&request.element);

    info!(
        "[PERF][HTTP][{}] /api/element/inspect start window_hash={:016x} {} max_depth={} max_nodes={} format={}",
        request_id,
        window_hash,
        element_meta,
        request.max_depth,
        request.max_nodes,
        request.format
    );

    let window = request.window.clone();
    let element = request.element.clone();
    let max_depth = request.max_depth;
    let max_nodes = request.max_nodes;
    let format = request.format.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_inspect(request_id, window, element, max_depth, max_nodes, format)
    })
    .await;

    match result {
        Ok(Ok(inspect_result)) => {
            info!(
                "[PERF][HTTP][{}] /api/element/inspect done status=ok success={} total_children={} duration_ms={}",
                request_id,
                inspect_result.success,
                inspect_result.total_children,
                request_start.elapsed().as_millis()
            );
            let api_nodes: Option<InspectNodeInfo> = inspect_result.nodes.map(Into::into);
            let flat_nodes: Vec<FlatInspectNodeInfo> = inspect_result.flat_nodes.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(InspectResponse {
                success: inspect_result.success,
                root_xpath: inspect_result.root_xpath,
                nodes: api_nodes,
                flat_nodes,
                filtered_nodes: vec![],
                text_output: inspect_result.text_output,
                total_children: inspect_result.total_children,
                error: inspect_result.error,
            })
        }
        Ok(Err(e)) => {
            warn!("Inspect element COM worker error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/inspect done status=com_error success=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::Ok().json(InspectResponse {
                success: false,
                root_xpath: request.element.clone(),
                nodes: None,
                flat_nodes: vec![],
                filtered_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Inspect element spawn error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/inspect done status=spawn_error success=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(InspectResponse {
                success: false,
                root_xpath: request.element.clone(),
                nodes: None,
                flat_nodes: vec![],
                filtered_nodes: vec![],
                text_output: None,
                total_children: 0,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

/// POST /api/element/navigate
/// Compass 导航：找到基准元素后逐步 TreeWalker 导航
pub async fn navigate_element(body: web::Json<NavigateRequest>) -> impl Responder {
    let request = body.into_inner();

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&request.window);
    let element_meta = xpath_meta(&request.element);

    info!(
        "[PERF][HTTP][{}] /api/element/navigate start window_hash={:016x} {} steps={}",
        request_id,
        window_hash,
        element_meta,
        request.steps.len()
    );

    let window = request.window.clone();
    let base_xpath = request.element.clone();
    let steps = request.steps.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::core::com_worker::global_navigate(request_id, window, base_xpath, steps)
    })
    .await;

    match result {
        Ok(Ok(Ok((Some(element_info), find_selector)))) => {
            info!(
                "Navigate succeeded: type='{}' name='{}'",
                element_info.control_type, element_info.name
            );
            info!(
                "[PERF][HTTP][{}] /api/element/navigate done status=ok found=true duration_ms={}",
                request_id,
                request_start.elapsed().as_millis()
            );
            HttpResponse::Ok().json(NavigateResponse {
                found: true,
                find_selector,
                element: Some(element_info),
                error: None,
            })
        }
        Ok(Ok(Ok((None, find_selector)))) => {
            warn!("Navigate: element not found at target position");
            info!(
                "[PERF][HTTP][{}] /api/element/navigate done status=ok found=false duration_ms={}",
                request_id,
                request_start.elapsed().as_millis()
            );
            HttpResponse::Ok().json(NavigateResponse {
                found: false,
                find_selector,
                element: None,
                error: Some("导航目标元素不存在".to_string()),
            })
        }
        Ok(Ok(Err(e))) => {
            warn!("Navigate failed: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/navigate done status=navigate_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::Ok().json(NavigateResponse {
                found: false,
                find_selector: String::new(),
                element: None,
                error: Some(e),
            })
        }
        Ok(Err(e)) => {
            warn!("Navigate COM worker error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/navigate done status=com_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(NavigateResponse {
                found: false,
                find_selector: String::new(),
                element: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Navigate spawn error: {}", e);
            warn!(
                "[PERF][HTTP][{}] /api/element/navigate done status=spawn_error found=false duration_ms={} error={}",
                request_id,
                request_start.elapsed().as_millis(),
                e
            );
            HttpResponse::InternalServerError().json(NavigateResponse {
                found: false,
                find_selector: String::new(),
                element: None,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}
