use super::*;
use uiautomation::patterns::{UIInvokePattern, UIValuePattern};

pub fn invoke_element_by_xpath(window_selector: &str, xpath: &str) -> anyhow::Result<Result<String, String>> {
    let auto = get_automation()?;

    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return Ok(Err(format!("窗口未找到: {}", window_selector)));
    }

    for window_element in &windows {
        match find_by_xpath_with_fallback(&auto, window_element, xpath) {
            Ok((elements, _)) => {
                if elements.is_empty() {
                    continue;
                }

                let elem = &elements[0];
                let elem_ct = elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                let elem_name = elem.get_name().unwrap_or_default();

                // 尝试获取 InvokePattern
                let invoke = match elem.get_pattern::<UIInvokePattern>() {
                    Ok(inv) => inv,
                    Err(e) => {
                        return Ok(Err(format!(
                            "元素 '{}' (type={}) 不支持 InvokePattern: {}",
                            elem_name, elem_ct, e
                        )));
                    }
                };

                // 执行 Invoke
                match invoke.invoke() {
                    Ok(()) => {
                        info!("UIA Invoke succeeded: element='{}' type={}", elem_name, elem_ct);
                        return Ok(Ok(format!("Invoke {} ({})", elem_name, elem_ct)));
                    }
                    Err(e) => {
                        return Ok(Err(format!(
                            "Invoke 执行失败: element='{}' type={} error={:?}",
                            elem_name, elem_ct, e
                        )));
                    }
                }
            }
            Err(e) => {
                log::debug!("XPath search failed on window: {}", e);
                continue;
            }
        }
    }

    Ok(Err("所有窗口均未找到匹配元素".to_string()))
}

pub fn focus_element_by_xpath(window_selector: &str, xpath: &str) -> anyhow::Result<Result<(), String>> {
    let auto = get_automation()?;

    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return Ok(Err(format!("窗口未找到: {}", window_selector)));
    }

    for window_element in &windows {
        // 先激活窗口
        if window_element.set_focus().is_err() {
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));

        match find_by_xpath_with_fallback(&auto, window_element, xpath) {
            Ok((elements, _)) => {
                if elements.is_empty() {
                    continue;
                }

                let elem = &elements[0];
                let elem_ct = elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                let elem_name = elem.get_name().unwrap_or_default();

                match elem.set_focus() {
                    Ok(()) => {
                        info!("UIA SetFocus succeeded: element='{}' type={}", elem_name, elem_ct);
                        return Ok(Ok(()));
                    }
                    Err(e) => {
                        return Ok(Err(format!(
                            "SetFocus 执行失败: element='{}' type={} error={:?}",
                            elem_name, elem_ct, e
                        )));
                    }
                }
            }
            Err(e) => {
                log::debug!("XPath search failed on window: {}", e);
                continue;
            }
        }
    }

    Ok(Err("所有窗口均未找到匹配元素".to_string()))
}

pub fn set_value_by_xpath(window_selector: &str, xpath: &str, value: &str) -> anyhow::Result<Result<usize, String>> {
    let auto = get_automation()?;

    let windows = find_window_by_selector(&auto, window_selector);
    if windows.is_empty() {
        return Ok(Err(format!("窗口未找到: {}", window_selector)));
    }

    for window_element in &windows {
        match find_by_xpath_with_fallback(&auto, window_element, xpath) {
            Ok((elements, _)) => {
                if elements.is_empty() {
                    continue;
                }

                let elem = &elements[0];
                let elem_ct = elem.get_control_type_raw().map(control_type_name).unwrap_or_default();
                let elem_name = elem.get_name().unwrap_or_default();

                // 获取 ValuePattern
                let value_pattern = match elem.get_pattern::<UIValuePattern>() {
                    Ok(vp) => vp,
                    Err(e) => {
                        return Ok(Err(format!(
                            "元素 '{}' (type={}) 不支持 ValuePattern: {}",
                            elem_name, elem_ct, e
                        )));
                    }
                };

                // 调用 SetValue
                let char_count = value.chars().count();
                match value_pattern.set_value(value) {
                    Ok(()) => {
                        info!(
                            "UIA ValuePattern.SetValue succeeded: '{}' → element='{}' type={}",
                            value, elem_name, elem_ct
                        );
                        return Ok(Ok(char_count));
                    }
                    Err(e) => {
                        return Ok(Err(format!(
                            "SetValue 执行失败: element='{}' type={} error={:?}",
                            elem_name, elem_ct, e
                        )));
                    }
                }
            }
            Err(e) => {
                log::debug!("XPath search failed on window: {}", e);
                continue;
            }
        }
    }

    Ok(Err("所有窗口均未找到匹配元素".to_string()))
}
