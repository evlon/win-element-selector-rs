// src/api/element.rs
//
// 元素查找 API

use std::time::Instant;

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::core::metrics::{next_request_id, selector_hash, xpath_meta};
use super::types::{ElementQuery, ElementResponse, ElementVisibilityRequest, ElementVisibilityResponse, ElementFlashRequest, ElementFlashResponse, Rect, InspectRequest, InspectResponse, InspectNodeInfo, FlatInspectNodeInfo, NavigateRequest, NavigateResponse, FindFromElementRequest, FindFromElementResponse};

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

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = element_query.runtime_id {
        info!(
            "[PERF][HTTP][{}] /api/element start (runtimeId) runtime_id={} window_hash={:016x}",
            request_id,
            runtime_id.len().min(32),
            window_hash
        );

        let rid = runtime_id.clone();
        let random_range = element_query.random_range;

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    let data = crate::core::uia::element_info_from_uia(
                        &elem, None, random_range, &mut rand::thread_rng()
                    );
                    match data {
                        Some(d) => (true, Some(d), None::<String>),
                        None => {
                            // COM proxy 已失效（元素被销毁），自动清除缓存
                            crate::core::element_cache::remove_cached_element(&rid);
                            (false, None, Some("无法读取元素属性（缓存已清除）".to_string()))
                        }
                    }
                }
                None => (false, None, Some(format!("元素不在缓存中: runtimeId={}", rid))),
            }
        }).await;

        match result {
            Ok((found, element_data, error)) => {
                let element_info: Option<super::types::ElementInfo> = element_data.map(Into::into);
                info!(
                    "[PERF][HTTP][{}] /api/element done (runtimeId) found={} duration_ms={}",
                    request_id, found, request_start.elapsed().as_millis()
                );
                HttpResponse::Ok().json(ElementResponse {
                    found,
                    element_selector: element_query.element.clone(),
                    element: element_info,
                    total: if found { 1 } else { 0 },
                    error,
                })
            }
            Err(e) => {
                warn!("[HTTP][{}] get_element (runtimeId) spawn error: {}", request_id, e);
                HttpResponse::InternalServerError().json(ElementResponse {
                    found: false,
                    element_selector: element_query.element.clone(),
                    element: None,
                    total: 0,
                    error: Some(format!("线程错误: {}", e)),
                })
            }
        }
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
        info!(
            "[PERF][HTTP][{}] /api/element start window_hash={:016x} {} random_range={} timeout_ms={:?}",
            request_id, window_hash, element_meta, element_query.random_range, element_query.timeout_ms
        );

        // Clone query for spawn_blocking (需要 'static)
        let window = element_query.window.clone();
        let mut element = element_query.element.clone();
        // If search_mode is explicitly set, append suffix (overrides any existing suffix)
        if let Some(ref mode) = element_query.search_mode {
            // Strip any existing suffix first
            let (_, stripped) = crate::core::model::SearchMode::strip_suffix(&element);
            let suffix = match mode {
                crate::core::model::SearchMode::All => "",
                crate::core::model::SearchMode::First => ":first",
                crate::core::model::SearchMode::OnlyOne => ":onlyone",
            };
            element = format!("{}{}", stripped, suffix);
        }
        let random_range = element_query.random_range;
        let search_context = element_query.search_context.clone();
        let timeout_ms = element_query.timeout_ms;
        let find_all_filter = element_query.find_all_filter.clone();
        let chrome_treewalker_fallback = element_query.chrome_treewalker_fallback;

        // Direct call to core::uia layer
        let result = tokio::task::spawn_blocking(move || {
            crate::core::uia::find_elements_by_xpath(&window, &element, random_range, search_context.as_ref(), timeout_ms, find_all_filter.as_ref(), chrome_treewalker_fallback)
        })
        .await;

        match result {
            Ok(elements) => {
                let total = elements.len();
                if let Some(element_data) = elements.into_iter().next() {
                    let center_pos = element_data.center
                        .map(|c| format!("({}, {})", c.x, c.y))
                        .unwrap_or_else(|| "N/A".to_string());
                    info!(
                        "Element found: type='{}' name='{}' center={} (total={})",
                        element_data.control_type, element_data.name, center_pos, total
                    );
                    info!(
                        "[PERF][HTTP][{}] /api/element done status=ok found=true total={} duration_ms={}",
                        request_id,
                        total,
                        request_start.elapsed().as_millis()
                    );
                    let element_info: super::types::ElementInfo = element_data.into();
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

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = element_query.runtime_id {
        info!(
            "[PERF][HTTP][{}] /api/element/all start (runtimeId) runtime_id={}",
            request_id,
            runtime_id.len().min(32)
        );

        let rid = runtime_id.clone();
        let random_range = element_query.random_range;

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    let data = crate::core::uia::element_info_from_uia(
                        &elem, None, random_range, &mut rand::thread_rng()
                    );
                    match data {
                        Some(d) => (true, vec![d], None::<String>),
                        None => {
                            // COM proxy 已失效（元素被销毁），自动清除缓存
                            crate::core::element_cache::remove_cached_element(&rid);
                            (false, vec![], Some("无法读取元素属性（缓存已清除）".to_string()))
                        }
                    }
                }
                None => (false, vec![], Some(format!("元素不在缓存中: runtimeId={}", rid))),
            }
        }).await;

        match result {
            Ok((found, elements, error)) => {
                info!(
                    "[PERF][HTTP][{}] /api/element/all done (runtimeId) found={} duration_ms={}",
                    request_id, found, request_start.elapsed().as_millis()
                );
                let elements_with_selector: Vec<ElementWithSelector> = elements
                    .into_iter()
                    .map(|el_data| ElementWithSelector {
                        element_selector: element_query.element.clone(),
                        info: el_data.into(),
                    })
                    .collect();
                let total = elements_with_selector.len();
                HttpResponse::Ok().json(AllElementsResponse {
                    found,
                    elements: elements_with_selector,
                    total,
                    error,
                })
            }
            Err(e) => {
                warn!("[HTTP][{}] get_all_elements (runtimeId) spawn error: {}", request_id, e);
                HttpResponse::InternalServerError().json(AllElementsResponse {
                    found: false,
                    elements: vec![],
                    total: 0,
                    error: Some(format!("线程错误: {}", e)),
                })
            }
        }
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
        info!(
            "[PERF][HTTP][{}] /api/element/all start window_hash={:016x} {} random_range={} timeout_ms={:?}",
            request_id, window_hash, element_meta, element_query.random_range, element_query.timeout_ms
        );

        let window = element_query.window.clone();
        let mut element = element_query.element.clone();
        // If search_mode is explicitly set, append suffix (overrides any existing suffix)
        if let Some(ref mode) = element_query.search_mode {
            let (_, stripped) = crate::core::model::SearchMode::strip_suffix(&element);
            let suffix = match mode {
                crate::core::model::SearchMode::All => "",
                crate::core::model::SearchMode::First => ":first",
                crate::core::model::SearchMode::OnlyOne => ":onlyone",
            };
            element = format!("{}{}", stripped, suffix);
        }
        let random_range = element_query.random_range;
        let search_context = element_query.search_context.clone();
        let timeout_ms = element_query.timeout_ms;
        let find_all_filter = element_query.find_all_filter.clone();
        let chrome_treewalker_fallback = element_query.chrome_treewalker_fallback;

        // Direct call to core::uia layer
        let result = tokio::task::spawn_blocking(move || {
            crate::core::uia::find_elements_by_xpath(&window, &element, random_range, search_context.as_ref(), timeout_ms, find_all_filter.as_ref(), chrome_treewalker_fallback)
        })
        .await;

        match result {
            Ok(elements) => {
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
                        .map(|el_data| ElementWithSelector {
                            element_selector: element_query.element.clone(),
                            info: el_data.into(),
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
}

/// POST /api/element/visibility
/// 获取元素在可视区域的位置信息
pub async fn get_element_visibility(body: web::Json<ElementVisibilityRequest>) -> impl Responder {
    let request = body.into_inner();

    let request_id = next_request_id();
    let request_start = Instant::now();
    let window_hash = selector_hash(&request.window);
    let element_meta = xpath_meta(&request.element);

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = request.runtime_id {
        info!(
            "[PERF][HTTP][{}] /api/element/visibility start (runtimeId) runtime_id={}",
            request_id,
            runtime_id.len().min(32)
        );

        let rid = runtime_id.clone();
        let container = request.container.clone();
        let window = request.window.clone();

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    crate::core::uia::get_element_visibility_by_elem(&elem, &window, container.as_deref())
                }
                None => {
                    crate::core::model::VisibilityResult {
                        found: false,
                        error: Some(format!("元素不在缓存中: runtimeId={}", rid)),
                        ..Default::default()
                    }
                }
            }
        }).await;

        match result {
            Ok(vis_result) => {
                let response = ElementVisibilityResponse {
                    found: vis_result.found,
                    is_offscreen: vis_result.is_offscreen,
                    visibility: vis_result.visibility.clone(),
                    position: vis_result.position.clone(),
                    element_rect: vis_result.element_rect,
                    visible_rect: vis_result.visible_rect,
                    viewport_rect: vis_result.viewport_rect,
                    overflow: vis_result.overflow,
                    scroll_direction: vis_result.scroll_direction,
                    error: vis_result.error,
                };
                info!(
                    "[PERF][HTTP][{}] /api/element/visibility done (runtimeId) found={} visibility={} duration_ms={}",
                    request_id, response.found, response.visibility, request_start.elapsed().as_millis()
                );
                HttpResponse::Ok().json(response)
            }
            Err(e) => {
                warn!("[HTTP][{}] get_element_visibility (runtimeId) spawn error: {}", request_id, e);
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
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
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
            crate::core::uia::get_element_visibility(&window, &element, container.as_deref())
        })
        .await;

        match result {
            Ok(vis_result) => {
                // Convert core::model::VisibilityResult → api::types::ElementVisibilityResponse
                let response = ElementVisibilityResponse {
                    found: vis_result.found,
                    is_offscreen: vis_result.is_offscreen,
                    visibility: vis_result.visibility.clone(),
                    position: vis_result.position.clone(),
                    element_rect: vis_result.element_rect,
                    visible_rect: vis_result.visible_rect,
                    viewport_rect: vis_result.viewport_rect,
                    overflow: vis_result.overflow,
                    scroll_direction: vis_result.scroll_direction,
                    error: vis_result.error,
                };
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
}

/// POST /api/element/flash
/// 在元素位置显示高亮闪烁，指定时间后自动消失
pub async fn flash_element(body: web::Json<ElementFlashRequest>) -> impl Responder {
    let request = body.into_inner();

    info!(
        "API: /api/element/flash window='{}' element='{}' timeout={}ms",
        request.window, request.element, request.timeout
    );

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = request.runtime_id {
        let rid = runtime_id.clone();
        let timeout = request.timeout;

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    let data = crate::core::uia::element_info_from_uia(
                        &elem, None, 0.0, &mut rand::thread_rng()
                    );
                    match data {
                        Some(d) => (true, d.rect.map(|r| (r.x, r.y, r.width, r.height)), d.control_type, None::<String>),
                        None => {
                            // COM proxy 已失效（元素被销毁），自动清除缓存
                            crate::core::element_cache::remove_cached_element(&rid);
                            (false, None, String::new(), Some("无法读取元素属性（缓存已清除）".to_string()))
                        }
                    }
                }
                None => (false, None, String::new(), Some(format!("元素不在缓存中: runtimeId={}", rid))),
            }
        }).await;

        match result {
            Ok((_found, rect_info, control_type, error)) => {
                if let Some((x, y, w, h)) = rect_info {
                    let model_rect = crate::core::model::ElementRect { x, y, width: w, height: h };
                    let info = crate::core::model::HighlightInfo::new(model_rect, &control_type);
                    crate::highlight::flash_with_info(&info, timeout);
                    let api_rect: Rect = crate::core::model::ElementRect { x, y, width: w, height: h }.into();
                    HttpResponse::Ok().json(ElementFlashResponse {
                        success: true,
                        element_rect: Some(api_rect),
                        error: None,
                    })
                } else {
                    HttpResponse::Ok().json(ElementFlashResponse {
                        success: false,
                        element_rect: None,
                        error: error.or(Some("元素无矩形区域信息".to_string())),
                    })
                }
            }
            Err(e) => {
                warn!("Flash element (runtimeId) spawn error: {}", e);
                HttpResponse::InternalServerError().json(ElementFlashResponse {
                    success: false,
                    element_rect: None,
                    error: Some(format!("线程错误: {}", e)),
                })
            }
        }
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
        let window = request.window.clone();
        let element = request.element.clone();
        let timeout = request.timeout;
        let random_range = request.random_range;

        // 查找元素获取其矩形区域
        let result = tokio::task::spawn_blocking(move || {
            crate::core::uia::find_elements_by_xpath(&window, &element, random_range, None, None, None, true)
        })
        .await;

        match result {
            Ok(elements) => {
                if let Some(element_data) = elements.into_iter().next() {
                    if let Some(rect) = &element_data.rect {
                        let model_rect = crate::core::model::ElementRect {
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                        };
                        let info = crate::core::model::HighlightInfo::new(
                            model_rect,
                            &element_data.control_type,
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

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = request.runtime_id {
        let rid = runtime_id.clone();
        let window = request.window.clone();
        let max_depth = request.max_depth;
        let max_nodes = request.max_nodes;
        let format = request.format.clone();

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    crate::core::uia::inspect_subtree_from_elem(&elem, &window, max_depth, max_nodes, &format)
                }
                None => {
                    crate::core::uia::InspectResult {
                        success: false,
                        root_xpath: String::new(),
                        nodes: None,
                        flat_nodes: vec![],
                        text_output: None,
                        total_children: 0,
                        error: Some(format!("元素不在缓存中: runtimeId={}", rid)),
                    }
                }
            }
        }).await;

        match result {
            Ok(inspect_result) => {
                info!(
                    "[PERF][HTTP][{}] /api/element/inspect done (runtimeId) success={} total_children={} duration_ms={}",
                    request_id, inspect_result.success, inspect_result.total_children, request_start.elapsed().as_millis()
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
            Err(e) => {
                warn!("[HTTP][{}] inspect_element (runtimeId) spawn error: {}", request_id, e);
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
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
        let window = request.window.clone();
        let element = request.element.clone();
        let max_depth = request.max_depth;
        let max_nodes = request.max_nodes;
        let format = request.format.clone();

        let result = tokio::task::spawn_blocking(move || {
            crate::core::uia::inspect_subtree(&window, &element, max_depth, max_nodes, &format)
        })
        .await;

        match result {
            Ok(inspect_result) => {
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

    // ══════════════════════════════════════════════════════════════
    // 路径 A: runtimeId 缓存优先（无 XPath fallback）
    // ══════════════════════════════════════════════════════════════
    if let Some(ref runtime_id) = request.runtime_id {
        let rid = runtime_id.clone();
        let window = request.window.clone();
        let steps = request.steps.clone();

        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    crate::core::uia::navigate_from_element_cached(elem, &window, &steps)
                }
                None => {
                    Err(format!("元素不在缓存中: runtimeId={}", rid))
                }
            }
        }).await;

        match result {
            Ok(Ok((Some(element_data), find_selector))) => {
                info!(
                    "[PERF][HTTP][{}] /api/element/navigate done (runtimeId) found=true duration_ms={}",
                    request_id, request_start.elapsed().as_millis()
                );
                let element_info: super::types::ElementInfo = element_data.into();
                HttpResponse::Ok().json(NavigateResponse {
                    found: true,
                    find_selector,
                    element: Some(element_info),
                    error: None,
                })
            }
            Ok(Ok((None, find_selector))) => {
                info!(
                    "[PERF][HTTP][{}] /api/element/navigate done (runtimeId) found=false duration_ms={}",
                    request_id, request_start.elapsed().as_millis()
                );
                HttpResponse::Ok().json(NavigateResponse {
                    found: false,
                    find_selector,
                    element: None,
                    error: Some("导航目标元素不存在".to_string()),
                })
            }
            Ok(Err(e)) => {
                warn!("[HTTP][{}] navigate (runtimeId) error: {}", request_id, e);
                HttpResponse::Ok().json(NavigateResponse {
                    found: false,
                    find_selector: String::new(),
                    element: None,
                    error: Some(e),
                })
            }
            Err(e) => {
                warn!("[HTTP][{}] navigate (runtimeId) spawn error: {}", request_id, e);
                HttpResponse::InternalServerError().json(NavigateResponse {
                    found: false,
                    find_selector: String::new(),
                    element: None,
                    error: Some(format!("线程错误: {}", e)),
                })
            }
        }
    } else {
        // ══════════════════════════════════════════════════════════════
        // 路径 B: 无 runtimeId → XPath 搜索（原有逻辑）
        // ══════════════════════════════════════════════════════════════
        let window = request.window.clone();
        let base_xpath = request.element.clone();
        let steps = request.steps.clone();

        let result = tokio::task::spawn_blocking(move || {
            crate::core::uia::navigate_from_element(&window, &base_xpath, &steps)
        })
        .await;

        match result {
            Ok(Ok((Some(element_data), find_selector))) => {
                info!(
                    "Navigate succeeded: type='{}' name='{}'",
                    element_data.control_type, element_data.name
                );
                info!(
                    "[PERF][HTTP][{}] /api/element/navigate done status=ok found=true duration_ms={}",
                    request_id,
                    request_start.elapsed().as_millis()
                );
                let element_info: super::types::ElementInfo = element_data.into();
                HttpResponse::Ok().json(NavigateResponse {
                    found: true,
                    find_selector,
                    element: Some(element_info),
                    error: None,
                })
            }
            Ok(Ok((None, find_selector))) => {
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
            Ok(Err(e)) => {
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
}

// ═══════════════════════════════════════════════════════════════════════════════
// XPath Cache Management API
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath cache statistics response
#[derive(Debug, Serialize, Deserialize)]
pub struct XPathCacheStatsResponse {
    pub entry_count: usize,
    pub total_hits: u64,
    pub cleared: bool,
}

/// GET /api/xpath-cache/stats
/// Returns XPath compilation cache statistics.
pub async fn get_xpath_cache_stats() -> impl Responder {
    let (count, hits) = crate::core::uia::xpath_cache_stats();
    HttpResponse::Ok().json(XPathCacheStatsResponse {
        entry_count: count,
        total_hits: hits,
        cleared: false,
    })
}

/// POST /api/xpath-cache/clear
/// Clears the XPath compilation cache.
pub async fn clear_xpath_cache_handler() -> impl Responder {
    crate::core::uia::clear_xpath_cache();
    log::info!("[API] XPath cache cleared");
    HttpResponse::Ok().json(XPathCacheStatsResponse {
        entry_count: 0,
        total_hits: 0,
        cleared: true,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Find-From-Element API
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/element/find-from
/// Find elements by XPath starting from a previously cached element.
/// This avoids re-searching the entire window tree — the parent element is
/// looked up from the element cache by its RuntimeId, then XPath search
/// runs only within its subtree.
///
/// Request body:
/// ```json
/// {
///   "runtimeId": "42,1234567890,1",   // from previous ElementInfo.runtimeId
///   "xpath": "//Text[@Name='标题']",
///   "randomRange": 0.0
/// }
/// ```
pub async fn find_from_element(body: web::Json<FindFromElementRequest>) -> impl Responder {
    let request_id = next_request_id();
    let request_start = Instant::now();

    info!(
        "[PERF][HTTP][{}] /api/element/find-from runtime_id={} xpath_len={}",
        request_id,
        body.runtime_id.len().min(32),  // Don't log full runtime_id (may be long)
        body.xpath.len(),
    );

    let runtime_id = body.runtime_id.clone();
    let xpath = body.xpath.clone();
    let search_strategy = body.search_strategy.clone().unwrap_or(crate::core::model::SearchStrategy::Adaptive);

    // 确定搜索模式：显式 search_mode > XPath 后缀 > 默认 First
    let effective_mode = body.search_mode.unwrap_or_else(|| {
        let (mode, _) = crate::core::model::SearchMode::strip_suffix(&xpath);
        mode
    });

    let result = tokio::task::spawn_blocking(move || {
        match effective_mode {
            crate::core::model::SearchMode::First => {
                let (elem, reason) = crate::core::uia::locate_first_from(&runtime_id, &xpath, search_strategy);
                (elem.into_iter().collect::<Vec<_>>(), reason)
            }
            crate::core::model::SearchMode::OnlyOne => {
                let (elem, reason) = crate::core::uia::locate_one_from(&runtime_id, &xpath, search_strategy);
                (elem.into_iter().collect::<Vec<_>>(), reason)
            }
            crate::core::model::SearchMode::All => {
                let elems = crate::core::uia::locate_all_from(&runtime_id, &xpath, search_strategy, None);
                (elems, None)
            }
        }
    })
    .await;

    match result {
        Ok((element_data_list, not_found_reason)) => {
            let total = element_data_list.len();
            info!(
                "[PERF][HTTP][{}] /api/element/find-from done found={} total={} duration_ms={}",
                request_id,
                total > 0,
                total,
                request_start.elapsed().as_millis()
            );
            let elements: Vec<super::types::ElementInfo> = element_data_list.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(FindFromElementResponse {
                found: total > 0,
                elements,
                total,
                error: None,
                not_found_reason,
            })
        }
        Err(e) => {
            warn!("[HTTP][{}] find-from-element task failed: {:?}", request_id, e);
            HttpResponse::InternalServerError().json(FindFromElementResponse {
                found: false,
                elements: vec![],
                total: 0,
                error: Some(format!("Internal error: {}", e)),
                not_found_reason: None,
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element Cache Refresh API (runtimeId-based, no XPath fallback)
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/element/refresh 请求
#[derive(Debug, Deserialize)]
pub struct RefreshByRuntimeIdRequest {
    pub window: String,
    #[serde(rename = "runtimeId")]
    pub runtime_id: String,
}

/// POST /api/element/refresh 响应
#[derive(Debug, Serialize)]
pub struct RefreshByRuntimeIdResponse {
    pub found: bool,
    pub element: Option<super::types::ElementInfo>,
    pub error: Option<String>,
}

/// POST /api/element/refresh
/// 通过 runtimeId 从缓存获取最新元素信息（无 XPath fallback）。
/// 缓存未命中或过期 → found=false。
pub async fn refresh_by_runtime_id(
    body: web::Json<RefreshByRuntimeIdRequest>,
) -> impl Responder {
    let request_id = next_request_id();
    let request_start = Instant::now();

    info!(
        "[PERF][HTTP][{}] /api/element/refresh runtime_id={}",
        request_id,
        body.runtime_id.len().min(32)
    );

    let runtime_id = body.runtime_id.clone();
    let _window = body.window.clone();

    let result = tokio::task::spawn_blocking(move || {
        match crate::core::element_cache::get_cached_element(&runtime_id) {
            Some(elem) => {
                // Read latest attributes from cached UIElement
                let data = crate::core::uia::element_info_from_uia(
                    &elem, None, 0.0, &mut rand::thread_rng()
                );
                match data {
                    Some(d) => (true, Some(d), None),
                    None => {
                        // COM proxy 已失效（元素被销毁），自动清除缓存
                        crate::core::element_cache::remove_cached_element(&runtime_id);
                        (false, None, Some("无法读取元素属性（缓存已清除）".to_string()))
                    }
                }
            }
            None => (false, None, Some("元素不在缓存中".to_string())),
        }
    }).await;

    match result {
        Ok((found, element_data, error)) => {
            let element_info: Option<super::types::ElementInfo> = element_data.map(Into::into);
            info!(
                "[PERF][HTTP][{}] /api/element/refresh done found={} duration_ms={}",
                request_id,
                found,
                request_start.elapsed().as_millis()
            );
            HttpResponse::Ok().json(RefreshByRuntimeIdResponse {
                found,
                element: element_info,
                error,
            })
        }
        Err(e) => {
            warn!("[HTTP][{}] refresh_by_runtime_id spawn error: {}", request_id, e);
            HttpResponse::InternalServerError().json(RefreshByRuntimeIdResponse {
                found: false,
                element: None,
                error: Some(format!("线程错误: {}", e)),
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element Cache Control API
// ═══════════════════════════════════════════════════════════════════════════════

/// PUT /api/element/cache/config 请求
#[derive(Debug, Deserialize)]
pub struct CacheConfigRequest {
    /// 全局缓存 TTL（毫秒），null = 永不过期
    #[serde(default, rename = "cacheTime")]
    pub cache_ttl_ms: Option<u64>,
}

/// GET /api/element/cache/stats 响应
#[derive(Debug, Serialize)]
pub struct CacheStatsResponse {
    pub size: usize,
    #[serde(rename = "maxSize")]
    pub max_size: usize,
    #[serde(rename = "defaultCacheTime")]
    pub default_ttl_ms: Option<u64>,
}

/// PUT /api/element/cache/config
/// 设置全局缓存 TTL（毫秒），null = 永不过期。
pub async fn set_cache_config(
    body: web::Json<CacheConfigRequest>,
) -> impl Responder {
    info!(
        "API: /api/element/cache/config cacheTime={:?}ms",
        body.cache_ttl_ms
    );

    crate::core::element_cache::set_default_ttl(
        body.cache_ttl_ms.map(std::time::Duration::from_millis)
    );

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

/// GET /api/element/cache/stats
/// 获取缓存统计信息。
pub async fn get_cache_stats() -> impl Responder {
    let (size, max_size, ttl) = crate::core::element_cache::cache_stats();
    HttpResponse::Ok().json(CacheStatsResponse {
        size,
        max_size,
        default_ttl_ms: ttl.map(|d| d.as_millis() as u64),
    })
}

/// POST /api/element/cache/clear
/// 清除所有元素缓存。
pub async fn clear_element_cache() -> impl Responder {
    crate::core::element_cache::clear_cache();
    log::info!("[API] Element cache cleared");
    HttpResponse::Ok().json(serde_json::json!({ "cleared": true }))
}
