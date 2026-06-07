// src/api/mouse.rs
//
// 鼠标操作 API

use actix_web::{web, HttpResponse, Responder};
use log::{info, warn};

use super::types::{
    MouseMoveRequest, MouseMoveResponse, MouseMoveOptions,
    MouseClickRequest, MouseClickResponse, MouseClickOptions, ClickMode,
    MouseScrollRequest, MouseScrollResponse, MouseScrollOptions,
    MouseHoverRequest, MouseHoverResponse, MouseHoverOptions,
    MouseDragRequest, MouseDragResponse, MouseDragOptions,
    MouseScrollDetectRequest, MouseScrollDetectResponse,
    ViewportInset,
    Point,
};
use super::super::mouse_control;
use super::idle_motion::with_auto_pause;

/// 从 serde_json::Value 中提取 xpath 字符串列表
/// 支持字符串和字符串数组两种形式：
/// - "xpath1" → vec!["xpath1"]
/// - ["xpath1", "xpath2"] → vec!["xpath1", "xpath2"]
fn extract_wait_xpaths(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        }
        _ => {
            warn!("wait field expected string or array of strings, got: {}", value);
            vec![]
        }
    }
}

/// 检查多个 wait xpath，返回第一个找到的结果
/// 任意一个 xpath 匹配即视为成功（OR 语义）
fn check_wait_xpaths(
    window_selector: &str,
    xpaths: &[String],
) -> Option<super::super::model::DetailedValidationResult> {
    for xpath in xpaths {
        let result = crate::core::uia::validate_selector_and_xpath_detailed(
            window_selector,
            xpath,
            &[],
            None, None, true,
        );
        if matches!(result.overall, super::super::model::ValidationResult::Found { .. }) {
            return Some(result);
        }
    }
    None
}

/// 将 viewportInset 应用到容器 rect 上，返回扣减后的有效视口 rect
/// 如果 inset 导致 width 或 height <= 0，返回 None（完全被遮挡）
fn apply_viewport_inset(container_rect: &Option<super::types::Rect>, inset: &Option<ViewportInset>) -> Option<super::types::Rect> {
    match (container_rect, inset) {
        (Some(vp), Some(ins)) => {
            // 解析各方向值，将百分比转换为像素
            let left_px = ins.left.as_ref().map(|v| v.resolve(vp.width)).unwrap_or(0).max(0);
            let top_px = ins.top.as_ref().map(|v| v.resolve(vp.height)).unwrap_or(0).max(0);
            let right_px = ins.right.as_ref().map(|v| v.resolve(vp.width)).unwrap_or(0).max(0);
            let bottom_px = ins.bottom.as_ref().map(|v| v.resolve(vp.height)).unwrap_or(0).max(0);

            let new_x = vp.x + left_px;
            let new_y = vp.y + top_px;
            let new_width = vp.width.saturating_sub(left_px + right_px);
            let new_height = vp.height.saturating_sub(top_px + bottom_px);

            if new_width > 0 && new_height > 0 {
                Some(super::types::Rect { x: new_x, y: new_y, width: new_width, height: new_height })
            } else {
                None // 完全被遮挡
            }
        }
        _ => container_rect.clone(), // 无 inset → 原样返回
    }
}

/// 计算元素 rect 与容器视口 rect 的交集（visible_rect）
/// inset 会先对 container_rect 做向内裁剪，排除固定遮挡区域
fn compute_visible_rect(
    element_rect: &Option<super::types::Rect>,
    container_rect: &Option<super::types::Rect>,
    viewport_inset: &Option<ViewportInset>,
) -> Option<super::types::Rect> {
    let effective_container = apply_viewport_inset(container_rect, viewport_inset);
    match (element_rect, effective_container) {
        (Some(er), Some(vp)) => {
            let left = er.x.max(vp.x);
            let top = er.y.max(vp.y);
            let right = (er.x + er.width).min(vp.x + vp.width);
            let bottom = (er.y + er.height).min(vp.y + vp.height);
            if right > left && bottom > top {
                Some(super::types::Rect { x: left, y: top, width: right - left, height: bottom - top })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// POST /api/mouse/move
/// 拟人化移动鼠标到目标坐标
pub async fn move_mouse(body: web::Json<MouseMoveRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseMoveOptions::default());

    info!(
        "API: /api/mouse/move target=({}, {}) humanize={} movePath={} duration={}ms",
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
/// 拟人化点击元素。支持三种点击模式（通过 options.clickMode 控制）：
///
/// - `mouse` (默认): 模拟鼠标点击，移动鼠标到元素位置通过 SendInput 点击
/// - `invoke`: 使用 UIA InvokePattern.Invoke() 触发点击，不受覆盖层影响
/// - `setFocus`: 通过 UIA SetFocus() 聚焦元素（适用于输入框等）
/// - `auto`: 自动选择最优策略，优先级: Invoke → SetFocus → 坐标点击
///
/// 启用 `occlusionCheck` 后，坐标点击前会通过 ElementFromPoint 检查目标位置
/// 是否被其他元素遮挡，检测到遮挡时返回错误。
pub async fn click_mouse(body: web::Json<MouseClickRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseClickOptions::default());

    let window_display = match &request.window {
        super::types::WindowSelectorOrString::String(s) => s.as_str().to_string(),
        super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or("").to_string(),
    };

    info!(
        "API: /api/mouse/click window='{}' element='{}' mode={:?} humanize={} checkBlocked={} runtime_id={:?}",
        window_display, request.element, options.click_mode, options.humanize, options.check_blocked, request.runtime_id
    );

    // Step 1: 构建窗口选择器
    let window_selector = build_window_selector(&request.window);
    let window_selector_for_click = window_selector.clone();

    let runtime_id = request.runtime_id.clone();

    // Step 2: 获取元素坐标
    // 有 runtimeId → 走缓存（未命中直接报错）
    // 无 runtimeId → 直接走 XPath 搜索
    if let Some(ref rid) = runtime_id {
            // Path A: runtimeId 缓存 → 获取 rect 用于坐标点击模式
            let rid_c = rid.clone();
            let rect_result = tokio::task::spawn_blocking(move || {
                match crate::core::element_cache::get_cached_element(&rid_c) {
                    Some(elem) => {
                        let rect = elem.get_bounding_rectangle().ok();
                        rect.map(|r| super::types::Rect {
                            x: r.get_left(),
                            y: r.get_top(),
                            width: r.get_width(),
                            height: r.get_height(),
                        })
                    }
                    None => None,
                }
            }).await;

            match rect_result {
                Ok(Some(rect_api)) => {
                    let element_name = request.element.clone();
                    return execute_click_by_mode(
                        &window_selector_for_click,
                        &request.element,
                        &rect_api,
                        &element_name,
                        &options,
                        Some(rid),
                    ).await;
                }
                _ => {
                    // 缓存未命中 → 直接报错，不 fallback
                    warn!("Cache miss for runtimeId={}, no fallback to XPath", rid);
                    return HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        click_method: None,
                        occlusion_detected: None,
                        occlusion_info: None,
                        error: Some(format!("元素不在缓存中: runtimeId={}", rid)),
                    });
                }
            }
        }
    // 无 runtimeId → 走 XPath 搜索
    // Path B: XPath 搜索
    let element = request.element.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        crate::core::uia::validate_selector_and_xpath_detailed(
            &window_selector,
            &element,
            &[], None, None, true,
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
                        let element_name = request.element.clone();

                        return execute_click_by_mode(
                            &window_selector_for_click,
                            &request.element,
                            &rect_api,
                            &element_name,
                            &options,
                            None,
                        ).await;
                    } else {
                        return HttpResponse::Ok().json(MouseClickResponse {
                            success: false,
                            click_point: Point::new(0, 0),
                            element: None,
                            click_method: None,
                            occlusion_detected: None,
                            occlusion_info: None,
                            error: Some("元素坐标获取失败".to_string()),
                        });
                    }
                }
                ValidationResult::NotFound { .. } => {
                    return HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        click_method: None,
                        occlusion_detected: None,
                        occlusion_info: None,
                        error: Some(format!(
                            "未找到匹配元素 (耗时 {}ms)",
                            detailed_result.total_duration_ms
                        )),
                    });
                }
                ValidationResult::Error(e) => {
                    return HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        click_method: None,
                        occlusion_detected: None,
                        occlusion_info: None,
                        error: Some(e.clone()),
                    });
                }
                _ => {
                    return HttpResponse::Ok().json(MouseClickResponse {
                        success: false,
                        click_point: Point::new(0, 0),
                        element: None,
                        click_method: None,
                        occlusion_detected: None,
                        occlusion_info: None,
                        error: Some("校验状态未知".to_string()),
                    });
                }
            }
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: None,
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

/// 根据 clickMode 分发到不同的点击策略
async fn execute_click_by_mode(
    window_selector: &str,
    element_xpath: &str,
    rect: &super::types::Rect,
    element_name: &str,
    options: &MouseClickOptions,
    runtime_id: Option<&str>,
) -> HttpResponse {
    info!("execute_click_by_mode: click_mode={:?}, rect=({},{},{}x{}), runtime_id={:?}",
        options.click_mode, rect.x, rect.y, rect.width, rect.height, runtime_id);
    match options.click_mode {
        ClickMode::Invoke => {
            click_via_invoke(window_selector, element_xpath, element_name, options, runtime_id).await
        }
        ClickMode::SetFocus => {
            click_via_set_focus(window_selector, element_xpath, element_name, options, runtime_id).await
        }
        ClickMode::Mouse => {
            click_via_mouse(window_selector, rect, element_name, options).await
        }
        ClickMode::Auto => {
            // 自动策略：Invoke → SetFocus → 坐标点击
            info!("Auto mode: trying Invoke first for element '{}'", element_name);

            // 尝试 Invoke
            let invoke_success = try_invoke_blocking(window_selector, element_xpath, runtime_id).await;
            if invoke_success {
                info!("Auto mode: Invoke succeeded");
                return HttpResponse::Ok().json(MouseClickResponse {
                    success: true,
                    click_point: Point::new(0, 0),
                    element: Some(super::types::ClickedElement {
                        control_type: "Element".to_string(),
                        name: element_name.to_string(),
                    }),
                    click_method: Some("auto->invoke".to_string()),
                    occlusion_detected: None,
                    occlusion_info: None,
                    error: None,
                });
            }
            info!("Auto mode: Invoke failed, trying SetFocus");

            // 尝试 SetFocus
            let focus_success = try_focus_blocking(window_selector, element_xpath, runtime_id).await;
            if focus_success {
                info!("Auto mode: SetFocus succeeded");
                return HttpResponse::Ok().json(MouseClickResponse {
                    success: true,
                    click_point: Point::new(0, 0),
                    element: Some(super::types::ClickedElement {
                        control_type: "Element".to_string(),
                        name: element_name.to_string(),
                    }),
                    click_method: Some("auto->setFocus".to_string()),
                    occlusion_detected: None,
                    occlusion_info: None,
                    error: None,
                });
            }
            info!("Auto mode: SetFocus failed, falling back to mouse click");

            // 回退到坐标点击
            click_via_mouse(window_selector, rect, element_name, options).await
        }
    }
}

/// 尝试 Invoke（非 HTTP 版本，供 auto 模式内部使用）
async fn try_invoke_blocking(
    window_selector: &str,
    element_xpath: &str,
    runtime_id: Option<&str>,
) -> bool {
    let ws = window_selector.to_string();
    let xp = element_xpath.to_string();
    let rid = runtime_id.map(|s| s.to_string());
    match tokio::task::spawn_blocking(move || {
        super::super::core::uia::invoke_element_by_xpath(&ws, &xp, rid.as_deref())
    }).await {
        Ok(Ok(Ok(_))) => true,
        _ => false,
    }
}

/// 尝试 SetFocus（非 HTTP 版本，供 auto 模式内部使用）
async fn try_focus_blocking(
    window_selector: &str,
    element_xpath: &str,
    runtime_id: Option<&str>,
) -> bool {
    let ws = window_selector.to_string();
    let xp = element_xpath.to_string();
    let rid = runtime_id.map(|s| s.to_string());
    match tokio::task::spawn_blocking(move || {
        super::super::core::uia::focus_element_by_xpath(&ws, &xp, rid.as_deref())
    }).await {
        Ok(Ok(Ok(()))) => true,
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 策略1: UIA InvokePattern.Invoke()
// ═══════════════════════════════════════════════════════════════════════════════

/// 通过 UIA InvokePattern 触发元素点击，不依赖坐标，不受覆盖层影响。
/// 适用于 Button、Hyperlink、MenuItem、ListItem、TabItem 等支持 InvokePattern 的控件。
async fn click_via_invoke(
    window_selector: &str,
    element_xpath: &str,
    element_name: &str,
    _options: &MouseClickOptions,
    runtime_id: Option<&str>,
) -> HttpResponse {
    let ws = window_selector.to_string();
    let xp = element_xpath.to_string();
    let name = element_name.to_string();
    let rid = runtime_id.map(|s| s.to_string());

    let result = tokio::task::spawn_blocking(move || {
        super::super::core::uia::invoke_element_by_xpath(&ws, &xp, rid.as_deref())
    }).await;

    match result {
        Ok(Ok(invoke_result)) => {
            info!("Invoke succeeded for element '{}': {:?}", name, invoke_result);
            HttpResponse::Ok().json(MouseClickResponse {
                success: true,
                click_point: Point::new(0, 0), // Invoke 无坐标
                element: Some(super::types::ClickedElement {
                    control_type: "Element".to_string(),
                    name,
                }),
                click_method: Some("invoke".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: None,
            })
        }
        Ok(Err(e)) => {
            warn!("Invoke failed for element '{}': {}", name, e);
            HttpResponse::Ok().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("invoke".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("Invoke 失败: {}", e)),
            })
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("invoke".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 策略2: UIA SetFocus
// ═══════════════════════════════════════════════════════════════════════════════

/// 通过 UIA SetFocus() 聚焦元素，适用于输入框等需要获取焦点的控件。
/// 不依赖坐标，不受覆盖层影响。
async fn click_via_set_focus(
    window_selector: &str,
    element_xpath: &str,
    element_name: &str,
    _options: &MouseClickOptions,
    runtime_id: Option<&str>,
) -> HttpResponse {
    let ws = window_selector.to_string();
    let xp = element_xpath.to_string();
    let name = element_name.to_string();
    let rid = runtime_id.map(|s| s.to_string());

    let result = tokio::task::spawn_blocking(move || {
        super::super::core::uia::focus_element_by_xpath(&ws, &xp, rid.as_deref())
    }).await;

    match result {
        Ok(Ok(Ok(()))) => {
            info!("SetFocus succeeded for element '{}'", name);
            HttpResponse::Ok().json(MouseClickResponse {
                success: true,
                click_point: Point::new(0, 0), // SetFocus 无坐标
                element: Some(super::types::ClickedElement {
                    control_type: "Element".to_string(),
                    name,
                }),
                click_method: Some("setFocus".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: None,
            })
        }
        Ok(Ok(Err(e))) => {
            warn!("SetFocus failed for element '{}': {}", name, e);
            HttpResponse::Ok().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("setFocus".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("SetFocus 失败: {}", e)),
            })
        }
        Ok(Err(e)) => {
            HttpResponse::InternalServerError().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("setFocus".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("UIA 错误: {:?}", e)),
            })
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("setFocus".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("内部错误: {}", e)),
            })
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 策略3: 坐标点击（带可选的遮挡检测）
// ═══════════════════════════════════════════════════════════════════════════════

/// 传统坐标点击，移动鼠标到元素位置通过 SendInput 点击。
/// 如果启用了 occlusionCheck，点击前会检查目标位置是否被遮挡。
async fn click_via_mouse(
    window_selector: &str,
    rect: &super::types::Rect,
    element_name: &str,
    options: &MouseClickOptions,
) -> HttpResponse {
    let ws = window_selector.to_string();
    let rect_api = rect.clone();
    let options_copy = options.clone();
    let element_name_copy = element_name.to_string();

    // 获取窗口矩形用于计算 visibleRect
    let window_rect = tokio::task::spawn_blocking(move || {
        super::super::core::uia::get_window_rect_by_selector(&ws)
    }).await.unwrap_or(None);

    let visible_rect = if let Some(win_rect) = window_rect {
        let win_rect_api = super::types::Rect {
            x: win_rect.x,
            y: win_rect.y,
            width: win_rect.width,
            height: win_rect.height,
        };
        let left = rect_api.x.max(win_rect_api.x);
        let top = rect_api.y.max(win_rect_api.y);
        let right = (rect_api.x + rect_api.width).min(win_rect_api.x + win_rect_api.width);
        let bottom = (rect_api.y + rect_api.height).min(win_rect_api.y + win_rect_api.height);
        if right > left && bottom > top {
            Some(super::types::Rect { x: left, y: top, width: right - left, height: bottom - top })
        } else {
            None
        }
    } else {
        None
    };

    // 计算随机点击点
    let click_point = match calculate_offset_click_point(&rect_api, &visible_rect, &options_copy) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::Ok().json(MouseClickResponse {
                success: false,
                click_point: Point::new(0, 0),
                element: None,
                click_method: Some("mouse".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("计算点击坐标失败: {}", e)),
            });
        }
    };

    // 遮挡检测（仅坐标点击模式下启用）
    if options_copy.check_blocked {
        let cp = click_point;
        let ws_check = window_selector.to_string();
        let occlusion_result = tokio::task::spawn_blocking(move || {
            check_occlusion_at_point(&ws_check, cp.x, cp.y)
        }).await;

        match occlusion_result {
            Ok(Ok(false)) => {
                // 无遮挡，继续
                info!("Occlusion check passed at ({}, {})", cp.x, cp.y);
            }
            Ok(Ok(true)) => {
                warn!("Occlusion detected at ({}, {}): target element is covered", cp.x, cp.y);
                return HttpResponse::Ok().json(MouseClickResponse {
                    success: false,
                    click_point: cp,
                    element: None,
                    click_method: Some("mouse".to_string()),
                    occlusion_detected: Some(true),
                    occlusion_info: Some("目标元素被其他层遮挡，无法通过鼠标点击。建议使用 clickMode='invoke' 或 'setFocus' 或 'auto'".to_string()),
                    error: Some("遮挡检测失败: 目标位置被覆盖层遮挡".to_string()),
                });
            }
            Ok(Err(e)) => {
                warn!("Occlusion check error: {}, continuing with click", e);
                // 检测出错时继续点击（不阻塞）
            }
            Err(e) => {
                warn!("Occlusion check spawn error: {}, continuing with click", e);
            }
        }
    }

    // 执行拟人化移动和点击
    let ws_for_click = window_selector.to_string();
    let click_point_copy = click_point;
    let options_copy2 = options_copy.clone();
    let name = element_name_copy.clone();

    with_auto_pause(|| async {
        // 确保窗口在前台
        info!("Activating window before click: {}", ws_for_click);
        let activated = super::super::core::uia::activate_window_by_selector(&ws_for_click);
        if !activated {
            warn!("Failed to activate window, but continuing...");
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let start_point = mouse_control::get_cursor_position();

        let move_result = if options_copy2.humanize {
            mouse_control::humanized_move(start_point, click_point_copy, 600, "bezier")
        } else {
            mouse_control::linear_move(start_point, click_point_copy)
        };

        if let Err(e) = move_result {
            warn!("Move to click position failed: {}", e);
            return HttpResponse::Ok().json(MouseClickResponse {
                success: false,
                click_point: click_point_copy,
                element: None,
                click_method: Some("mouse".to_string()),
                occlusion_detected: None,
                occlusion_info: None,
                error: Some(format!("移动失败: {}", e)),
            });
        }

        if options_copy2.pause_before > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(options_copy2.pause_before)).await;
        }

        let click_result = if options_copy2.button == "right" {
            mouse_control::right_click_at(click_point_copy)
        } else {
            mouse_control::click_at(click_point_copy)
        };

        if options_copy2.pause_after > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(options_copy2.pause_after)).await;
        }

        match click_result {
            Ok(_) => {
                info!("Click executed successfully at ({}, {})", click_point_copy.x, click_point_copy.y);

                if options_copy2.show_dot {
                    crate::highlight::flash_point(click_point_copy.x, click_point_copy.y, options_copy2.dot_duration);
                }

                HttpResponse::Ok().json(MouseClickResponse {
                    success: true,
                    click_point: click_point_copy,
                    element: Some(super::types::ClickedElement {
                        control_type: "Element".to_string(),
                        name,
                    }),
                    click_method: Some("mouse".to_string()),
                    occlusion_detected: None,
                    occlusion_info: None,
                    error: None,
                })
            }
            Err(e) => {
                warn!("Click failed: {}", e);
                HttpResponse::Ok().json(MouseClickResponse {
                    success: false,
                    click_point: click_point_copy,
                    element: None,
                    click_method: Some("mouse".to_string()),
                    occlusion_detected: None,
                    occlusion_info: None,
                    error: Some(format!("点击失败: {}", e)),
                })
            }
        }
    }).await
}

/// 遮挡检测：通过 ElementFromPoint 检查指定坐标处的顶层元素是否与目标窗口匹配。
///
/// 返回 Ok(true) 表示检测到遮挡，Ok(false) 表示无遮挡。
fn check_occlusion_at_point(_window_selector: &str, x: i32, y: i32) -> anyhow::Result<bool> {
    let _request_id = super::super::core::metrics::next_request_id();
    // 获取坐标处的元素 rect
    let rect_result = crate::core::uia::get_element_rect_at_point(x, y);

    match rect_result {
        Some(rect) => {
            // 有返回值说明 ElementFromPoint 找到了元素
            // 进一步检查：这个元素的 BoundingRectangle 是否包含我们的目标点
            // 如果包含，说明顶层元素就在这个位置，不算严格意义上的遮挡
            // 但我们无法判断这个顶层元素是不是我们想要的，所以这里返回 false（允许继续）
            // 真正的遮挡判断在 capture_enhanced 层做更准确
            info!("Occlusion check at ({}, {}): ElementFromPoint returned rect {:?}", x, y, rect);
            // 简单策略：如果 ElementFromPoint 能返回元素，且该元素包含我们的点，
            // 我们不把它当作遮挡（因为可能就是目标窗口内的元素）
            // 真正的跨窗口遮挡需要通过窗口 Z-order 判断，这里只做基本检查
            Ok(false)
        }
        None => {
            // ElementFromPoint 返回空，可能是坐标无效
            warn!("Occlusion check at ({}, {}): ElementFromPoint returned None", x, y);
            Ok(false)
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

/// 计算带偏移的随机点击点（与 visibleRect 二次校验）
/// 
/// 流程：
/// 1. 根据 offset/clickArea/randomRange 计算基准区域
/// 2. 与 visibleRect 求交集，得到最终可点击区域
/// 3. 在最终区域内完全随机选择坐标
/// 
/// 优先级：offset > clickArea > randomRange
fn calculate_offset_click_point(
    rect: &super::types::Rect,
    visible_rect: &Option<super::types::Rect>,
    options: &super::types::MouseClickOptions
) -> Result<Point, String> {
    use rand::Rng;
    use super::types::{ClickOffset, PresetOffset};

    // 前置校验：rect 无效时返回友好错误，防止后续 gen_range 对空区间 panic
    if rect.width <= 0 || rect.height <= 0 {
        return Err(format!(
            "元素不可见或不在可视区域内 (bounds: ({}, {}, {}x{})) — 元素可能被遮挡、已滚动出视图或尚未渲染",
            rect.x, rect.y, rect.width, rect.height
        ));
    }
    
    // Step 1: 确定基准区域（基于 offset/clickArea/randomRange）
    let (base_left, base_right, base_top, base_bottom) = if let Some(ref offset) = options.offset {
        // 使用 offset 配置
        match offset {
            ClickOffset::Preset(preset) => {
                // 预设位置：基于边界附近的小区域
                match preset {
                    PresetOffset::Top => {
                        let margin = 10.0;
                        (rect.x as f32, (rect.x + rect.width) as f32, 
                         rect.y as f32, (rect.y as f32 + margin))
                    },
                    PresetOffset::Bottom => {
                        let margin = 10.0;
                        (rect.x as f32, (rect.x + rect.width) as f32, 
                         (rect.y + rect.height) as f32 - margin, (rect.y + rect.height) as f32)
                    },
                    PresetOffset::Left => {
                        let margin = 10.0;
                        (rect.x as f32, rect.x as f32 + margin,
                         rect.y as f32, (rect.y + rect.height) as f32)
                    },
                    PresetOffset::Right => {
                        let margin = 10.0;
                        ((rect.x + rect.width) as f32 - margin, (rect.x + rect.width) as f32,
                         rect.y as f32, (rect.y + rect.height) as f32)
                    },
                    PresetOffset::Center => {
                        // center 时使用 clickArea 或 randomRange
                        if let Some(ref area) = options.click_area {
                            calculate_effective_bounds(rect, area)
                        } else {
                            let range = options.random_range;
                            let cx = rect.center().x as f32;
                            let cy = rect.center().y as f32;
                            let hw = rect.width as f32 * range / 2.0;
                            let hh = rect.height as f32 * range / 2.0;
                            (cx - hw, cx + hw, cy - hh, cy + hh)
                        }
                    }
                }
            },
            ClickOffset::Expression(expr) => {
                // 解析表达式得到偏移量
                match crate::api::offset_parser::parse_offset_expression(expr, rect) {
                    Ok((offset_x, offset_y)) => {
                        // 以元素左上角为基准，应用偏移
                        let base_x = rect.x as f32 + offset_x;
                        let base_y = rect.y as f32 + offset_y;
                        // 在偏移点周围创建小范围随机区域（±10px）
                        (base_x - 10.0, base_x + 10.0, base_y - 10.0, base_y + 10.0)
                    },
                    Err(e) => {
                        log::warn!("Failed to parse offset expression '{}': {}, fallback to center", expr, e);
                        // 降级到 center 行为
                        let range = options.random_range;
                        let cx = rect.center().x as f32;
                        let cy = rect.center().y as f32;
                        let hw = rect.width as f32 * range / 2.0;
                        let hh = rect.height as f32 * range / 2.0;
                        (cx - hw, cx + hw, cy - hh, cy + hh)
                    }
                }
            }
        }
    } else if let Some(ref area) = options.click_area {
        // 使用 clickArea
        calculate_effective_bounds(rect, area)
    } else {
        // 使用 randomRange（旧行为）
        let range = options.random_range;
        let cx = rect.center().x as f32;
        let cy = rect.center().y as f32;
        let hw = rect.width as f32 * range / 2.0;
        let hh = rect.height as f32 * range / 2.0;
        (cx - hw, cx + hw, cy - hh, cy + hh)
    };
    
    // Step 2: 与 visibleRect 求交集（二次校验）
    let (final_left, final_right, final_top, final_bottom) = if let Some(ref vr) = visible_rect {
        let vr_left = vr.x as f32;
        let vr_right = (vr.x + vr.width) as f32;
        let vr_top = vr.y as f32;
        let vr_bottom = (vr.y + vr.height) as f32;
        
        // 求交集
        let intersect_left = base_left.max(vr_left);
        let intersect_right = base_right.min(vr_right);
        let intersect_top = base_top.max(vr_top);
        let intersect_bottom = base_bottom.min(vr_bottom);
        
        // 检查交集是否有效
        if intersect_right > intersect_left && intersect_bottom > intersect_top {
            (intersect_left, intersect_right, intersect_top, intersect_bottom)
        } else {
            // 交集为空，说明 offset 指定的区域完全不可见
            log::warn!("Offset region has no intersection with visibleRect, using visibleRect as fallback");
            // 降级：直接在 visibleRect 内随机
            (vr_left, vr_right, vr_top, vr_bottom)
        }
    } else {
        // 无 visibleRect 信息，直接使用基准区域
        (base_left, base_right, base_top, base_bottom)
    };
    
    // Step 3: 在最终区域内完全随机选择坐标
    // 安全兜底：最终区间无效时返回友好错误
    if final_left >= final_right || final_top >= final_bottom {
        return Err(format!(
            "无法计算有效点击坐标 — 目标区域无效 (x: {:.1}..{:.1}, y: {:.1}..{:.1})。元素可能完全不可见",
            final_left, final_right, final_top, final_bottom
        ));
    }
    let mut rng = rand::thread_rng();
    let x = rng.gen_range(final_left..final_right) as i32;
    let y = rng.gen_range(final_top..final_bottom) as i32;
    
    Ok(Point::new(x, y))
}

/// 辅助函数：计算 clickArea 的有效边界
fn calculate_effective_bounds(rect: &super::types::Rect, area: &super::types::ClickArea) -> (f32, f32, f32, f32) {
    let left = area.left.unwrap_or(0.0);
    let right = area.right.unwrap_or(0.0);
    let top = area.top.unwrap_or(0.0);
    let bottom = area.bottom.unwrap_or(0.0);

    let eff_left = rect.x as f32 + rect.width as f32 * left;
    let eff_right = rect.x as f32 + rect.width as f32 * (1.0 - right);
    let eff_top = rect.y as f32 + rect.height as f32 * top;
    let eff_bottom = rect.y as f32 + rect.height as f32 * (1.0 - bottom);
    (eff_left, eff_right, eff_top, eff_bottom)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/scroll
/// 滚动鼠标滚轮，边滚边检测 wait xpath
pub async fn scroll_mouse(body: web::Json<MouseScrollRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseScrollOptions::default());
    let times = options.times.unwrap_or(100);
    let delta = options.delta.unwrap_or(120);
    let timeout_ms = options.timeout.unwrap_or(5000);
    let auto_delta = options.auto_delta.unwrap_or(false);
    let _delta_factor = options.delta_factor.unwrap_or(0.8); // 保留参数声明，auto_delta 模式下使用
    let wait_visible = options.wait_mode.as_deref() == Some("visible");
    let scroll_to_center = options.scroll_to_center.unwrap_or(true);
    let _scroll_to_center_adjust_times = options.scroll_to_center_adjust_times.unwrap_or(5); // 黄金微调模式下由 golden_adjust_max_steps 替代
    let viewport_inset = options.viewport_inset.clone();
    // 平滑滚动参数
    let smooth_step_delta = options.smooth_step_delta.unwrap_or(40);
    let smooth_step_delay_ms = options.smooth_step_delay_ms.unwrap_or(200);
    let golden_ratio = options.golden_ratio.unwrap_or(0.0); // 0.0 表示自动选择
    let golden_adjust_max_steps = options.golden_adjust_max_steps.unwrap_or(10);
    // 优先使用传入的窗口选择器，否则回退到通用 "Window"
    let window_selector = request.window.as_deref().unwrap_or("Window").to_string();
    let window_selector_for_wait = window_selector.clone();

    info!(
        "API: /api/mouse/scroll window='{}' element='{}' times={} delta={} smooth_step={}/{}ms golden_ratio={} auto_delta={} wait={:?} wait_mode={:?} timeout={}ms",
        window_selector, request.element, times, delta, smooth_step_delta, smooth_step_delay_ms, golden_ratio, auto_delta, options.wait, options.wait_mode, timeout_ms
    );

    // Step 1: 获取元素坐标
    let element_for_query = request.element.clone();
    let window_selector_for_element = window_selector.clone();
    let element_result = tokio::task::spawn_blocking(move || {
        crate::core::uia::validate_selector_and_xpath_detailed(
            &window_selector_for_element,
            &element_for_query,
            &[],
            None, None, true,
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
                visible_rect: None,
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
                    visible_rect: None,
                    scrolled_to_end: false,
                    error: Some("元素坐标获取失败".to_string()),
                });
            }
        }
        ValidationResult::NotFound { .. } => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                visible_rect: None,
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
                visible_rect: None,
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
                visible_rect: None,
                scrolled_to_end: false,
                error: Some("校验状态未知".to_string()),
            });
        }
    };

    // ── 黄金比例目标 Y ──
    // 自动选择：下滚→0.618(黄金分割下段)，上滚→0.382(黄金分割上段)
    // 手动指定 golden_ratio > 0 则使用该值
    let golden_target_ratio: f32 = if golden_ratio > 0.0 {
        golden_ratio
    } else if delta < 0 {
        0.618 // 向下滚动 → 目标在视口 61.8% 处（黄金分割）
    } else {
        0.382 // 向上滚动 → 目标在视口 38.2% 处（黄金分割补位）
    };
    // 用滚动容器的 rect 计算黄金目标 Y
    let viewport_golden_y: Option<i32> = if scroll_to_center {
        container_rect.as_ref().map(|r| {
            r.y + (r.height as f32 * golden_target_ratio) as i32
        })
    } else {
        None
    };
    if let Some(vy) = viewport_golden_y {
        info!("Golden target Y = {} (ratio={:.3}, container height={})", vy, golden_target_ratio, container_rect.as_ref().map(|r| r.height).unwrap_or(0));
    } else if scroll_to_center {
        warn!("scrollToCenter enabled but golden target Y unavailable");
    }

    let _container_height_for_auto = container_rect.as_ref().map(|r| r.height); // auto_delta 模式下使用
    // 容器视口高度，用于百分比计算（threshold、delta 缩放等都基于此）
    // 必须从 UIA 获取，不允许硬编码降级——不同屏幕/DPI 下差异巨大
    let container_height = match container_rect.as_ref() {
        Some(r) => r.height as f32,
        None => {
            return HttpResponse::Ok().json(MouseScrollResponse {
                success: false,
                scrolled: 0,
                target_found: false,
                target_rect: None,
                visible_rect: None,
                scrolled_to_end: false,
                error: Some("容器 rect 无数据，无法计算视口高度。请换一个有效的滚动容器元素（确保 UIA 可获取其 BoundingRectangle）".to_string()),
            });
        }
    };

    // Step 1.5: 预检查 wait 元素是否已完全可见（在容器视口内且非 offscreen）
    // 如果已可见，直接返回，避免无意义的滚动和鼠标移动
    if let Some(ref wait_value) = options.wait {
        if wait_visible {
            let precheck_xpaths = extract_wait_xpaths(wait_value);
            let precheck_window = window_selector.clone();
            let precheck_container_rect = container_rect.clone();
            let precheck_result = tokio::task::spawn_blocking(move || {
                check_wait_xpaths(&precheck_window, &precheck_xpaths)
            })
            .await;

            if let Ok(Some(pr)) = precheck_result {
                if matches!(pr.overall, ValidationResult::Found { .. }) {
                    let cur_offscreen = pr.is_offscreen.unwrap_or(false);
                    if !cur_offscreen {
                        // 检查元素 rect 是否完全在容器 rect 内（考虑 viewportInset）
                        let effective_precheck_container = apply_viewport_inset(&precheck_container_rect, &viewport_inset);
                        let fully_visible = match (&effective_precheck_container, &pr.overall) {
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
                            let visible_rect = compute_visible_rect(&target_rect, &precheck_container_rect, &viewport_inset);
                            return HttpResponse::Ok().json(MouseScrollResponse {
                                success: true,
                                scrolled: 0,
                                target_found: true,
                                target_rect,
                                visible_rect,
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

        // Step 3: 平滑滚动 + UIA 实时检测 + 黄金高度微调
        let mut scrolled: u32 = 0;
        let start_time = std::time::Instant::now();

        // ── 滚动步长 ──
        // smooth_step_delta=0 时使用原始 delta（向后兼容旧行为）
        let step_delta = if smooth_step_delta > 0 { smooth_step_delta * delta.signum() } else { delta };
        let step_delay_ms = if smooth_step_delta > 0 { smooth_step_delay_ms } else { options.scroll_interval_ms.unwrap_or(1000) };

        // ── auto_delta 初始滚动（保留兼容） ──
        if auto_delta && step_delta.abs() == delta.abs() {
            // 先滚动一次固定 delta 以获得实际容器高度
            let _ = mouse_control::scroll_wheel(delta);
            scrolled += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(options.auto_delta_initial_delay_ms.unwrap_or(1000))).await;
            // 重新获取容器高度
            // （autoDelta 模式下后续使用自适应步长，此处跳过重算以简化）
            info!("Auto delta initial scroll done, step_delta={}", step_delta);
        }

        // ── 滚动到底检测 ──
        const STUCK_CHECK_INTERVAL: u32 = 10;
        const STUCK_Y_THRESHOLD: i32 = 2;
        const STUCK_COUNT_THRESHOLD: u32 = 2;
        let mut prev_element_y: Option<i32> = None;
        let mut stuck_count: u32 = 0;
        let check_x = scroll_point.x;
        let check_y = scroll_point.y;

        // ── Phase 1: 主循环 — 平滑小步滚动直到找到目标或超时/到底 ──
        while scrolled < times {
            // 超时检测
            if start_time.elapsed().as_millis() as u64 >= timeout_ms {
                info!("Scroll timeout after {} steps", scrolled);
                return HttpResponse::Ok().json(MouseScrollResponse {
                    success: true,
                    scrolled,
                    target_found: false,
                    target_rect: None,
                    visible_rect: None,
                    scrolled_to_end: false,
                    error: Some(format!("超时 {}ms", timeout_ms)),
                });
            }

            // 检测 wait xpath
            let mut found_target = false;
            let mut target_rect: Option<super::types::Rect> = None;
            let mut cur_element_center_y: Option<i32> = None;

            if let Some(ref wait_value) = options.wait {
                let wait_xpaths = extract_wait_xpaths(wait_value);
                let win_sel = window_selector_for_wait.clone();
                let wait_result = tokio::task::spawn_blocking(move || {
                    check_wait_xpaths(&win_sel, &wait_xpaths)
                }).await;

                if let Ok(Some(wr)) = wait_result {
                    if matches!(wr.overall, ValidationResult::Found { .. }) {
                        target_rect = if let ValidationResult::Found { first_rect, .. } = &wr.overall {
                            first_rect.as_ref().map(|r| r.clone().into())
                        } else { None };
                        cur_element_center_y = if let ValidationResult::Found { first_rect, .. } = &wr.overall {
                            first_rect.as_ref().map(|r| r.y + r.height / 2)
                        } else { None };

                        let cur_offscreen = wr.is_offscreen.unwrap_or(false);

                        if wait_visible {
                            // ── 可见性判断 ──
                            let effectively_visible = if cur_offscreen {
                                let rect_valid = target_rect.as_ref().map_or(false, |r| r.width > 0 && r.height > 0);
                                if rect_valid {
                                    match (&container_rect, &target_rect) {
                                        (Some(vp), Some(er)) => {
                                            er.x < vp.x + vp.width && er.x + er.width > vp.x &&
                                            er.y < vp.y + vp.height && er.y + er.height > vp.y
                                        }
                                        _ => false,
                                    }
                                } else { false }
                            } else { true };

                            if effectively_visible {
                                let effective_container_for_sv = apply_viewport_inset(&container_rect, &viewport_inset);
                                let sufficiently_visible = match (&target_rect, &effective_container_for_sv) {
                                    (Some(er), Some(vp)) => {
                                        if er.height <= vp.height {
                                            er.y >= vp.y && er.y + er.height <= vp.y + vp.height
                                        } else {
                                            let vis_top = er.y.max(vp.y);
                                            let vis_bottom = (er.y + er.height).min(vp.y + vp.height);
                                            let vis_height = vis_bottom.saturating_sub(vis_top);
                                            vis_height >= (vp.height as f32 * 0.8) as i32
                                        }
                                    }
                                    _ => true,
                                };
                                found_target = sufficiently_visible;
                            }
                        } else {
                            // exist 模式：找到即成功
                            found_target = true;
                        }
                    }
                }
            }

            if found_target {
                info!("Target found after {} steps, entering golden ratio adjustment", scrolled);

                // ── Phase 2: 黄金高度微调 ──
                if scroll_to_center {
                    let mut golden_adjust_count: u32 = 0;
                    loop {
                        // 检查是否已在黄金位置附近
                        let ey = cur_element_center_y;
                        let gy = viewport_golden_y;
                        if let (Some(element_y), Some(golden_y)) = (ey, gy) {
                            let distance = (element_y - golden_y).abs() as f32;
                            let threshold = container_height * 0.05; // 5% 视口高度阈值
                            if distance <= threshold {
                                info!("Golden position reached: element_y={}, golden_y={}, distance={:.1}px (threshold={:.1}px)", element_y, golden_y, distance, threshold);
                                break;
                            }

                            if golden_adjust_count >= golden_adjust_max_steps {
                                info!("Golden adjust limit reached ({}/{}), element_y={}, golden_y={}",
                                    golden_adjust_count, golden_adjust_max_steps, element_y, golden_y);
                                break;
                            }

                            // 计算微调方向和步长
                            let direction_sign: i32 = if element_y > golden_y { -1 } else { 1 };
                            let distance_ratio = (distance / container_height).min(1.0).max(0.05);
                            let adjust_delta = (step_delta.abs() as f32 * distance_ratio) as i32;
                            let min_delta = step_delta.abs().min(40).max(10); // 最小 10，避免无限微调
                            let final_adjust = adjust_delta.max(min_delta) * direction_sign;

                            info!("Golden adjust {}/{}: element_y={}, golden_y={}, distance={:.1}px, ratio={:.3}, delta={}",
                                golden_adjust_count + 1, golden_adjust_max_steps, element_y, golden_y, distance, distance_ratio, final_adjust);

                            let _ = mouse_control::scroll_wheel(final_adjust);
                            scrolled += 1;
                            golden_adjust_count += 1;
                            tokio::time::sleep(tokio::time::Duration::from_millis(step_delay_ms)).await;

                            // 重新检测目标位置
                            if let Some(ref wait_value) = options.wait {
                                let wait_xpaths = extract_wait_xpaths(wait_value);
                                let win_sel = window_selector_for_wait.clone();
                                let wait_result = tokio::task::spawn_blocking(move || {
                                    check_wait_xpaths(&win_sel, &wait_xpaths)
                                }).await;
                                if let Ok(Some(wr)) = wait_result {
                                    if let ValidationResult::Found { first_rect, .. } = &wr.overall {
                                        target_rect = first_rect.as_ref().map(|r| r.clone().into());
                                        cur_element_center_y = first_rect.as_ref().map(|r| r.y + r.height / 2);
                                    }
                                }
                            }
                        } else {
                            // 无黄金位置数据，跳过微调
                            break;
                        }
                    }
                }

                let visible_rect = compute_visible_rect(&target_rect, &container_rect, &viewport_inset);
                info!("Scroll done: target found after {} steps, target_rect={:?}", scrolled, target_rect);
                return HttpResponse::Ok().json(MouseScrollResponse {
                    success: true,
                    scrolled,
                    target_found: true,
                    target_rect,
                    visible_rect,
                    scrolled_to_end: false,
                    error: None,
                });
            }

            // ── 执行小步滚动 ──
            let _ = mouse_control::scroll_wheel(step_delta);
            scrolled += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(step_delay_ms)).await;

            // ── 滚动到底检测 ──
            if scrolled > 0 && scrolled % STUCK_CHECK_INTERVAL == 0 {
                let cx = check_x;
                let cy = check_y;
                let rect_result = tokio::task::spawn_blocking(move || {
                    crate::core::uia::get_element_rect_at_point(cx, cy)
                }).await;

                if let Ok(Some(rect)) = rect_result {
                    let cur_y = rect.y;
                    if let Some(py) = prev_element_y {
                        if (cur_y - py).abs() <= STUCK_Y_THRESHOLD {
                            stuck_count += 1;
                            info!("Scroll stuck: prev_y={}, cur_y={}, stuck={}/{}", py, cur_y, stuck_count, STUCK_COUNT_THRESHOLD);
                            if stuck_count >= STUCK_COUNT_THRESHOLD {
                                info!("Scroll reached end after {} steps", scrolled);
                                return HttpResponse::Ok().json(MouseScrollResponse {
                                    success: true,
                                    scrolled,
                                    target_found: false,
                                    target_rect: None,
                                    visible_rect: None,
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
                    prev_element_y = None;
                    stuck_count = 0;
                }
            }
        }

        // 全部滚动完成但未找到目标
        info!("Scroll completed {} steps, target not found", scrolled);
        HttpResponse::Ok().json(MouseScrollResponse {
            success: true,
            scrolled,
            target_found: false,
            target_rect: None,
            visible_rect: None,
            scrolled_to_end: false,
            error: None,
        })
    }).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动边界检测
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/mouse/scroll-detect
/// 连续小幅滚动 + 持续 UIA 快照比对，检测是否到达滚动边界
///
/// 相比旧版"单次滚动+比对"，新版采用：
/// 1. 获取 container 坐标，移动鼠标到其中心
/// 2. 用 find_visible_elements 在容器内查询可见元素，记录快照
/// 3. 循环：小步滚动(delta=40) → 等待(200ms) → 再拍快照 → 比对位置变化
/// 4. 连续 stuck_threshold(默认3) 次快照中所有 watched 元素位置不变 → 判定到底
/// 5. 若 rollback=true，反向累计滚动恢复位置
pub async fn scroll_detect(body: web::Json<MouseScrollDetectRequest>) -> impl Responder {
    let request = body.into_inner();
    let direction = request.direction.to_lowercase();
    // 滚动方向符号：down=负(向下滚)，up=正(向上滚)
    let dir_sign: i32 = match direction.as_str() {
        "up" => 1,
        _ => -1, // 默认向下
    };
    let step_delta_abs = request.step_delta.max(10).min(120);
    let step_delta = step_delta_abs * dir_sign;
    let step_delay_ms = request.step_delay_ms.max(50).min(2000);
    let stuck_threshold = request.stuck_threshold.max(1).min(10);
    let max_steps = request.max_steps.max(1).min(100);
    let rollback = request.rollback;
    let window_selector = request.window.as_deref().unwrap_or("Window").to_string();
    let control_types = request.control_types.clone();
    let container_xpath = request.container.clone();
    let exclude_xpaths = request.exclude.clone();

    info!(
        "API: /api/mouse/scroll-detect window='{}' container='{}' control_types={:?} direction='{}' step_delta={} step_delay={}ms stuck_threshold={} max_steps={} exclude={:?} rollback={}",
        window_selector, container_xpath, control_types, direction, step_delta, step_delay_ms, stuck_threshold, max_steps, exclude_xpaths, rollback
    );

    // ── Helper: 构建错误响应 ──────────────────────────────────────────────
    fn err_resp(msg: &str, steps_scrolled: u32) -> HttpResponse {
        HttpResponse::Ok().json(MouseScrollDetectResponse {
            success: false,
            at_end: false,
            watched_count: 0,
            changed_count: 0,
            details: vec![],
            rolled_back: false,
            steps_scrolled,
            error: Some(msg.to_string()),
        })
    }

    // ── Step 1: 获取 container 坐标 → 移动鼠标 ───────────────────────────
    let container_for_query = container_xpath.clone();
    let window_for_container = window_selector.clone();
    let container_result = tokio::task::spawn_blocking(move || {
        crate::core::uia::validate_selector_and_xpath_detailed(
            &window_for_container,
            &container_for_query,
            &[],
            None, None, true,
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
                    return err_resp("滚动容器坐标获取失败", 0);
                }
            }
            ValidationResult::NotFound { .. } => {
                return err_resp(&format!("未找到滚动容器: {}", container_xpath), 0);
            }
            ValidationResult::Error(e) => {
                return err_resp(e, 0);
            }
            _ => {
                return err_resp("滚动容器校验状态未知", 0);
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
                steps_scrolled: 0,
                error: Some(format!("内部错误: {}", e)),
            });
        }
    };

    // ── Step 2: 在自动暂停上下文中执行检测 ───────────────────────────────
    with_auto_pause(|| async {
        // 移动鼠标到滚动容器中心
        let start_point = mouse_control::get_cursor_position();
        let _ = mouse_control::humanized_move(start_point, scroll_point, 400, "bezier");

        // ── 元素快照类型 ──────────────────────────────────────────────────
        #[derive(Clone)]
        struct ElementSnapshot {
            identifier: String,
            top: Option<i32>,
            is_offscreen: bool,
        }

        fn make_identifier(elem: &super::types::ElementInfo) -> String {
            if !elem.automation_id.is_empty() {
                format!("aid:{}", elem.automation_id)
            } else if !elem.name.is_empty() || !elem.class_name.is_empty() {
                format!("n:{}|c:{}", elem.name, elem.class_name)
            } else {
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
                let elem_info: super::types::ElementInfo = elem.clone().into();
                let identifier = make_identifier(&elem_info);
                let top = elem.rect.as_ref().map(|r| r.y);
                ElementSnapshot {
                    identifier,
                    top,
                    is_offscreen: elem.is_offscreen,
                }
            }).collect()
        }

        // ── Step 3: 查询 exclude 元素的标识集合（只做一次）────────────────
        let exclude_identifiers: std::collections::HashSet<String> = {
            let mut set = std::collections::HashSet::new();
            for ex_xpath in &exclude_xpaths {
                let ws = window_selector.clone();
                let xp = ex_xpath.clone();
                let ex_elements = tokio::task::spawn_blocking(move || {
                    crate::core::uia::find_all_elements_detailed(&ws, &xp, 0.0, None, None, None, true)
                }).await.unwrap_or_default();
                for elem_data in &ex_elements {
                    let elem_info: super::types::ElementInfo = elem_data.clone().into();
                    set.insert(make_identifier(&elem_info));
                }
            }
            set
        };

        // ── 快照比对函数：返回 (changed_count, watched_count, details) ──
        const TOP_THRESHOLD: i32 = 2;
        fn compare_snapshots(
            prev: &[ElementSnapshot],
            cur: &[ElementSnapshot],
            exclude: &std::collections::HashSet<String>,
        ) -> (usize, usize, Vec<super::types::ElementChangeDetail>) {
            let cur_map: std::collections::HashMap<String, &ElementSnapshot> = cur
                .iter()
                .map(|s| (s.identifier.clone(), s))
                .collect();
            let prev_ids: std::collections::HashSet<String> = prev
                .iter()
                .map(|s| s.identifier.clone())
                .collect();

            let mut watched = 0usize;
            let mut changed = 0usize;
            let mut details = Vec::new();

            for before in prev {
                if exclude.contains(&before.identifier) {
                    continue;
                }
                watched += 1;
                if let Some(after) = cur_map.get(&before.identifier) {
                    let top_changed = match (before.top, after.top) {
                        (Some(b), Some(a)) => (a - b).abs() > TOP_THRESHOLD,
                        _ => false,
                    };
                    let offscreen_changed = before.is_offscreen != after.is_offscreen;
                    if top_changed || offscreen_changed {
                        changed += 1;
                        details.push(super::types::ElementChangeDetail {
                            identifier: before.identifier.clone(),
                            before_top: before.top,
                            after_top: after.top,
                            delta_top: match (before.top, after.top) {
                                (Some(b), Some(a)) => Some(a - b),
                                _ => None,
                            },
                            offscreen_changed,
                        });
                    }
                } else {
                    changed += 1;
                    details.push(super::types::ElementChangeDetail {
                        identifier: before.identifier.clone(),
                        before_top: before.top,
                        after_top: None,
                        delta_top: None,
                        offscreen_changed: true,
                    });
                }
            }
            // 新出现元素
            for after in cur {
                if exclude.contains(&after.identifier) {
                    continue;
                }
                if !prev_ids.contains(&after.identifier) {
                    watched += 1;
                    changed += 1;
                    details.push(super::types::ElementChangeDetail {
                        identifier: after.identifier.clone(),
                        before_top: None,
                        after_top: after.top,
                        delta_top: None,
                        offscreen_changed: true,
                    });
                }
            }
            (changed, watched, details)
        }

        // ── Step 4: 初始快照 ──────────────────────────────────────────────
        let ws0 = window_selector.clone();
        let ct0 = container_xpath.clone();
        let ct_refs0 = control_types.clone();
        let mut prev_snapshot = tokio::task::spawn_blocking(move || {
            let ct_refs: Vec<&str> = ct_refs0.iter().map(|s| s.as_str()).collect();
            take_snapshot(&ws0, &ct0, &ct_refs)
        }).await.unwrap_or_default();

        info!("Scroll detect: initial snapshot {} visible elements", prev_snapshot.len());

        // ── Step 5: 连续小幅滚动 + 持续快照比对 ──────────────────────────
        let mut total_scrolled = 0u32;
        let mut stuck_count = 0u32;
        let mut final_at_end = false;
        let mut final_watched: usize = 0;
        let mut final_changed: usize = 0;
        let mut final_details: Vec<super::types::ElementChangeDetail> = Vec::new();

        while total_scrolled < max_steps {
            // 执行小步滚动
            let _ = mouse_control::scroll_wheel(step_delta);
            total_scrolled += 1;

            // 等待 UI 响应
            tokio::time::sleep(tokio::time::Duration::from_millis(step_delay_ms)).await;

            // 拍快照
            let ws = window_selector.clone();
            let ct = container_xpath.clone();
            let ct_refs = control_types.clone();
            let cur_snapshot = tokio::task::spawn_blocking(move || {
                let ct_refs: Vec<&str> = ct_refs.iter().map(|s| s.as_str()).collect();
                take_snapshot(&ws, &ct, &ct_refs)
            }).await.unwrap_or_default();

            // 比对
            let (changed, watched, details) = compare_snapshots(&prev_snapshot, &cur_snapshot, &exclude_identifiers);

            info!(
                "Scroll detect step {}/{}: prev={} cur={} watched={} changed={} stuck={}/{}",
                total_scrolled, max_steps, prev_snapshot.len(), cur_snapshot.len(), watched, changed, stuck_count, stuck_threshold
            );

            if watched > 0 && changed == 0 {
                // 所有 watched 元素位置不变 → stuck+1
                stuck_count += 1;
                if stuck_count >= stuck_threshold {
                    info!("Scroll detect: reached end after {} steps ({} consecutive stuck checks)", total_scrolled, stuck_count);
                    final_at_end = true;
                    final_watched = watched;
                    final_changed = changed;
                    final_details = details;
                    break;
                }
            } else {
                // 有变化 → 重置 stuck 计数
                stuck_count = 0;
            }

            // 保存当前快照作为下一轮的 prev
            prev_snapshot = cur_snapshot;
        }

        // 循环结束但未达到 stuck_threshold → 判定为未到底
        if total_scrolled >= max_steps && stuck_count < stuck_threshold {
            info!("Scroll detect: max steps reached ({}), not at end", total_scrolled);
            final_at_end = false;
            final_watched = 0;
            final_changed = 0;
            final_details.clear();
        }

        // ── Step 6: rollback 恢复位置（反向累计滚动） ─────────────────────
        let mut rolled_back = false;
        if rollback && total_scrolled > 0 {
            let rollback_delta = -step_delta * total_scrolled as i32;
            let _ = mouse_control::scroll_wheel(rollback_delta);
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            rolled_back = true;
            info!("Scroll detect rollback: total_steps={} rollback_delta={}", total_scrolled, rollback_delta);
        }

        HttpResponse::Ok().json(MouseScrollDetectResponse {
            success: true,
            at_end: final_at_end,
            watched_count: final_watched,
            changed_count: final_changed,
            details: final_details,
            rolled_back,
            steps_scrolled: total_scrolled,
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
        "API: /api/mouse/hover window='{}' element='{}' humanize={} duration={}ms runtime_id={:?}",
        match &request.window {
            super::types::WindowSelectorOrString::String(s) => s.as_str(),
            super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or(""),
        },
        request.element,
        options.humanize,
        options.duration,
        request.runtime_id,
    );

    let window_selector = build_window_selector(&request.window);

    // Path A: runtimeId 缓存优先
    if let Some(ref rid) = request.runtime_id {
        let rid_c = rid.clone();
        let rect_result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid_c) {
                Some(elem) => {
                    let rect = elem.get_bounding_rectangle().ok();
                    rect.map(|r| super::types::Rect {
                        x: r.get_left(),
                        y: r.get_top(),
                        width: r.get_width(),
                        height: r.get_height(),
                    })
                }
                None => None,
            }
        }).await;

        match rect_result {
            Ok(Some(rect_api)) => {
                let hover_point = rect_api.center();
                with_auto_pause(|| async {
                    let _ = super::super::core::uia::activate_window_by_selector(&build_window_selector(&request.window));
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    if options.humanize {
                        let start = mouse_control::get_cursor_position();
                        let _ = mouse_control::humanized_move(start, hover_point, options.duration / 2, "bezier");
                    } else {
                        let _ = mouse_control::linear_move(mouse_control::get_cursor_position(), hover_point);
                    }

                    tokio::time::sleep(tokio::time::Duration::from_millis(options.duration)).await;
                    info!("Hover completed at ({}, {})", hover_point.x, hover_point.y);
                    HttpResponse::Ok().json(MouseHoverResponse {
                        success: true,
                        hover_point,
                        error: None,
                    })
                }).await
            }
            _ => {
                HttpResponse::Ok().json(MouseHoverResponse {
                    success: false,
                    hover_point: Point::new(0, 0),
                    error: Some(format!("元素不在缓存中: runtimeId={}", rid)),
                })
            }
        }
    } else {
        // Path B: XPath 搜索（原有逻辑）
        let element = request.element.clone();
        let element_result = tokio::task::spawn_blocking(move || {
            crate::core::uia::validate_selector_and_xpath_detailed(
                &window_selector,
                &element,
                &[],
                None, None, true,
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
                    ValidationResult::NotFound { .. } => {
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
}

/// POST /api/mouse/drag
/// 从源元素拖拽到目标元素
pub async fn drag_mouse(body: web::Json<MouseDragRequest>) -> impl Responder {
    let request = body.into_inner();
    let options = request.options.unwrap_or(MouseDragOptions::default());

    info!(
        "API: /api/mouse/drag window='{}' source='{}' target='{}' duration={}ms source_rid={:?} target_rid={:?}",
        match &request.window {
            super::types::WindowSelectorOrString::String(s) => s.as_str(),
            super::types::WindowSelectorOrString::Object(obj) => obj.title.as_deref().unwrap_or(""),
        },
        request.source_element,
        request.target_element,
        options.duration,
        request.source_runtime_id,
        request.target_runtime_id,
    );

    let window_selector = build_window_selector(&request.window);

    // Helper: 从缓存获取元素 rect
    fn get_rect_from_cache(rid: &str) -> Option<super::types::Rect> {
        crate::core::element_cache::get_cached_element(rid).and_then(|elem| {
            elem.get_bounding_rectangle().ok().map(|r| super::types::Rect {
                x: r.get_left(),
                y: r.get_top(),
                width: r.get_width(),
                height: r.get_height(),
            })
        })
    }

    // Helper: 从 XPath 搜索获取 rect
    fn get_rect_from_xpath(ws: &str, xp: &str) -> Option<super::types::Rect> {
        let result = crate::core::uia::validate_selector_and_xpath_detailed(
            ws, xp, &[], None, None, true,
        );
        match result.overall {
            super::super::model::ValidationResult::Found { first_rect, .. } => {
                first_rect.map(|r| r.into())
            }
            _ => None,
        }
    }

    // 获取源元素 rect
    let source_rect: Option<super::types::Rect> = if let Some(ref rid) = request.source_runtime_id {
        get_rect_from_cache(rid)
    } else {
        let ws = window_selector.clone();
        let xp = request.source_element.clone();
        tokio::task::spawn_blocking(move || get_rect_from_xpath(&ws, &xp)).await.unwrap_or(None)
    };

    let source_point = match source_rect {
        Some(r) => r.center(),
        None => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point: Point::new(0, 0),
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(format!("未找到源元素: {}", request.source_element)),
            });
        }
    };

    // 获取目标元素 rect
    let target_rect: Option<super::types::Rect> = if let Some(ref rid) = request.target_runtime_id {
        get_rect_from_cache(rid)
    } else {
        let ws = window_selector.clone();
        let xp = request.target_element.clone();
        tokio::task::spawn_blocking(move || get_rect_from_xpath(&ws, &xp)).await.unwrap_or(None)
    };

    let target_point = match target_rect {
        Some(r) => r.center(),
        None => {
            return HttpResponse::Ok().json(MouseDragResponse {
                success: false,
                source_point,
                target_point: Point::new(0, 0),
                duration_ms: 0,
                error: Some(format!("未找到目标元素: {}", request.target_element)),
            });
        }
    };

    with_auto_pause(|| async {
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
