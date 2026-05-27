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

/// 元素信息附带其选择器（ElementInfo 属性直接扁平化，无嵌套）
#[derive(Debug, Serialize, Deserialize)]
pub struct ElementWithSelector {
    #[serde(rename = "elementSelector")]
    pub element_selector: String,
    pub rect: Rect,
    pub center: Point,
    #[serde(rename = "centerRandom")]
    pub center_random: Point,
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
    #[serde(rename = "automationId")]
    pub automation_id: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    #[serde(rename = "helpText")]
    pub help_text: String,
    #[serde(rename = "localizedControlType")]
    pub localized_control_type: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    #[serde(rename = "isPassword")]
    pub is_password: bool,
    #[serde(rename = "acceleratorKey")]
    pub accelerator_key: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "itemStatus")]
    pub item_status: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
    #[serde(default, rename = "isCheckable", skip_serializing_if = "Option::is_none")]
    pub is_checkable: Option<bool>,
    #[serde(default, rename = "isChecked", skip_serializing_if = "Option::is_none")]
    pub is_checked: Option<bool>,
    #[serde(default, rename = "isClickable", skip_serializing_if = "Option::is_none")]
    pub is_clickable: Option<bool>,
    #[serde(default, rename = "isScrollable", skip_serializing_if = "Option::is_none")]
    pub is_scrollable: Option<bool>,
    #[serde(default, rename = "isSelected", skip_serializing_if = "Option::is_none")]
    pub is_selected: Option<bool>,
}

use super::types::{Point, Rect};

/// 多元素查找响应（扁平化：elementSelector 与 ElementInfo 属性同级）
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
            if let Some(element_info) = elements.into_iter().next() {
                info!(
                    "Element found: type='{}' name='{}' center=({},{})",
                    element_info.control_type, element_info.name,
                    element_info.center.x, element_info.center.y
                );
                HttpResponse::Ok().json(ElementResponse {
                    found: true,
                    element_selector: element_query.element.clone(),
                    element: Some(element_info),
                    error: None,
                })
            } else {
                warn!("Element not found");
                HttpResponse::Ok().json(ElementResponse {
                    found: false,
                    element_selector: element_query.element.clone(),
                    element: None,
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
                error: Some(format!("内部错误: {}", e)),
            })
        }
        Err(e) => {
            warn!("Spawn blocking error: {}", e);
            HttpResponse::InternalServerError().json(ElementResponse {
                found: false,
                element_selector: element_query.element.clone(),
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
                        rect: el_info.rect,
                        center: el_info.center,
                        center_random: el_info.center_random,
                        control_type: el_info.control_type,
                        name: el_info.name,
                        automation_id: el_info.automation_id,
                        class_name: el_info.class_name,
                        framework_id: el_info.framework_id,
                        help_text: el_info.help_text,
                        localized_control_type: el_info.localized_control_type,
                        is_enabled: el_info.is_enabled,
                        is_offscreen: el_info.is_offscreen,
                        is_password: el_info.is_password,
                        accelerator_key: el_info.accelerator_key,
                        access_key: el_info.access_key,
                        item_type: el_info.item_type,
                        item_status: el_info.item_status,
                        process_id: el_info.process_id,
                        is_checkable: el_info.is_checkable,
                        is_checked: el_info.is_checked,
                        is_clickable: el_info.is_clickable,
                        is_scrollable: el_info.is_scrollable,
                        is_selected: el_info.is_selected,
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
