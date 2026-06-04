//! 测试：对比 ControlViewWalker 和 RawViewWalker 的祖先链差异
//!
//! 使用方法：
//!   cargo run --bin test-walker-compare
//!
//! 程序会：
//! 1. 读取当前鼠标位置
//! 2. 用 ElementFromPoint 获取元素
//! 3. 分别用 ControlViewWalker 和 RawViewWalker 构建祖先链（一直到 Desktop）
//! 4. 对比两条链的节点数量、类型，输出详细差异
//!
//! 预期：对于 Qt 程序，ControlViewWalker 应该过滤掉没有意义的 Group/Pane 容器节点，
//!       祖先链应该显著短于 RawViewWalker。

use std::sync::Once;
use uiautomation::types::Point;
use uiautomation::UIAutomation;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::Foundation::POINT;

static INIT: Once = Once::new();

fn init_logger() {
    INIT.call_once(|| {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format_timestamp_millis()
            .init();
    });
}

/// 获取全局 UIAutomation 实例
fn get_automation() -> anyhow::Result<UIAutomation> {
    UIAutomation::new().map_err(|e| anyhow::anyhow!("Failed to create UIAutomation: {e}"))
}

/// 将 ControlType 转换为可读名称
fn control_type_name(ct: u32) -> String {
    match ct {
        // 使用 uiautomation crate 中的常量
        50000 => "Button".into(),
        50004 => "Edit".into(),
        50005 => "Hyperlink".into(),
        50007 => "Image".into(),
        50008 => "ListItem".into(),
        50009 => "List".into(),
        50010 => "Menu".into(),
        50011 => "MenuBar".into(),
        50012 => "MenuItem".into(),
        50014 => "ProgressBar".into(),
        50015 => "RadioButton".into(),
        50018 => "ScrollBar".into(),
        50019 => "Slider".into(),
        50020 => "Spinner".into(),
        50021 => "StatusBar".into(),
        50022 => "Tab".into(),
        50023 => "TabItem".into(),
        50024 => "Text".into(),
        50025 => "ToolBar".into(),
        50026 => "ToolTip".into(),
        50027 => "Tree".into(),
        50028 => "TreeItem".into(),
        50029 => "Custom".into(),
        50030 => "Group".into(),
        50031 => "Thumb".into(),
        50032 => "DataGrid".into(),
        50033 => "DataItem".into(),
        50034 => "Document".into(),
        50035 => "SplitButton".into(),
        50036 => "Window".into(),
        50037 => "Pane".into(),
        50038 => "Header".into(),
        50039 => "HeaderItem".into(),
        50040 => "Table".into(),
        50041 => "TitleBar".into(),
        50042 => "Separator".into(),
        50043 => "SemanticZoom".into(),
        50044 => "AppBar".into(),
        other => format!("Unknown({})", other),
    }
}

/// 获取元素的属性信息（用于诊断）
fn describe_element(elem: &uiautomation::UIElement) -> String {
    let name = elem.get_name().unwrap_or_default();
    let ct = elem.get_control_type_raw()
        .map(|c| control_type_name(c as u32))
        .unwrap_or_else(|_| "N/A".into());
    let aid = elem.get_automation_id().unwrap_or_default();
    let class = elem.get_classname().unwrap_or_default();
    let framework = elem.get_framework_id().unwrap_or_default();
    let pid = elem.get_process_id().unwrap_or(0);
    let is_enabled = elem.is_enabled().unwrap_or(false);
    let is_control = elem.is_control_element().unwrap_or(false);
    let is_content = elem.is_content_element().unwrap_or(false);

    format!(
        "name='{}' ctrl='{}' aid='{}' class='{}' fw='{}' pid={} enabled={} isCtrl={} isContent={}",
        name, ct, aid, class, framework, pid, is_enabled, is_control, is_content
    )
}

fn main() -> anyhow::Result<()> {
    init_logger();

    // 1. 获取当前鼠标位置
    let pt = unsafe {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            anyhow::bail!("GetCursorPos 失败");
        }
        p
    };
    println!("============================================================");
    println!("  ControlView vs RawView 祖先链对比测试");
    println!("============================================================");
    println!("鼠标位置: ({}, {})", pt.x, pt.y);
    println!();

    // 2. 创建 UIAutomation
    let auto = get_automation()?;

    // 3. ElementFromPoint
    let point = Point::new(pt.x, pt.y);
    let hit_elem = auto.element_from_point(point)
        .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?;
    println!("[命中元素] {}", describe_element(&hit_elem));
    println!();

    // 4. 用 ControlViewWalker 构建祖先链
    let desktop = auto.get_root_element()?;

    println!("========== ControlViewWalker 祖先链 ==========");
    let ctrl_walker = auto.get_control_view_walker()
        .map_err(|e| anyhow::anyhow!("ControlViewWalker: {e}"))?;

    let mut ctrl_chain: Vec<uiautomation::UIElement> = vec![hit_elem.clone()];
    let mut current = ctrl_walker.get_parent(&hit_elem).ok();
    while let Some(elem) = current {
        let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
        ctrl_chain.push(elem.clone());
        if is_desktop { break; }
        current = ctrl_walker.get_parent(&elem).ok();
    }
    ctrl_chain.reverse();

    let mut ctrl_container_count = 0;
    for (i, node) in ctrl_chain.iter().enumerate() {
        let desc = describe_element(node);
        let ct = node.get_control_type_raw()
            .map(|c| control_type_name(c as u32))
            .unwrap_or_default();
        // 标记容器类型
        let is_container = matches!(ct.as_str(), "Group" | "Pane" | "Custom");
        if is_container {
            ctrl_container_count += 1;
        }
        println!("  [{}] {} {}",
            i,
            if is_container { "[CONTAINER]" } else { "           " },
            desc
        );
    }
    println!("  => 总计 {} 个节点，其中 {} 个容器节点", ctrl_chain.len(), ctrl_container_count);
    println!();

    // 5. 用 RawViewWalker 构建祖先链
    println!("========== RawViewWalker 祖先链 ==========");
    let raw_walker = auto.get_raw_view_walker()
        .map_err(|e| anyhow::anyhow!("RawViewWalker: {e}"))?;

    let mut raw_chain: Vec<uiautomation::UIElement> = vec![hit_elem.clone()];
    let mut current = raw_walker.get_parent(&hit_elem).ok();
    while let Some(elem) = current {
        let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
        raw_chain.push(elem.clone());
        if is_desktop { break; }
        current = raw_walker.get_parent(&elem).ok();
    }
    raw_chain.reverse();

    let mut raw_container_count = 0;
    for (i, node) in raw_chain.iter().enumerate() {
        let desc = describe_element(node);
        let ct = node.get_control_type_raw()
            .map(|c| control_type_name(c as u32))
            .unwrap_or_default();
        let is_container = matches!(ct.as_str(), "Group" | "Pane" | "Custom");
        if is_container {
            raw_container_count += 1;
        }
        println!("  [{}] {} {}",
            i,
            if is_container { "[CONTAINER]" } else { "           " },
            desc
        );
    }
    println!("  => 总计 {} 个节点，其中 {} 个容器节点", raw_chain.len(), raw_container_count);
    println!();

    // 5.5. 用 ContentViewWalker 构建祖先链（最严格的过滤器）
    println!("========== ContentViewWalker 祖先链 ==========");
    let content_walker = match auto.get_content_view_walker() {
        Ok(w) => {
            println!("  ContentViewWalker 获取成功");
            w
        }
        Err(e) => {
            println!("  ContentViewWalker 获取失败: {}", e);
            return Ok(());
        }
    };

    let mut content_chain: Vec<uiautomation::UIElement> = vec![hit_elem.clone()];
    let mut current = content_walker.get_parent(&hit_elem).ok();
    while let Some(elem) = current {
        let is_desktop = auto.compare_elements(&elem, &desktop).unwrap_or(false);
        content_chain.push(elem.clone());
        if is_desktop { break; }
        current = content_walker.get_parent(&elem).ok();
    }
    content_chain.reverse();

    let mut content_container_count = 0;
    for (i, node) in content_chain.iter().enumerate() {
        let desc = describe_element(node);
        let ct = node.get_control_type_raw()
            .map(|c| control_type_name(c as u32))
            .unwrap_or_default();
        let is_container = matches!(ct.as_str(), "Group" | "Pane" | "Custom");
        if is_container {
            content_container_count += 1;
        }
        println!("  [{}] {} {}",
            i,
            if is_container { "[CONTAINER]" } else { "           " },
            desc
        );
    }
    println!("  => 总计 {} 个节点，其中 {} 个容器节点", content_chain.len(), content_container_count);
    println!();

    // 6. 对比分析
    println!("============================================================");
    println!("  对比分析");
    println!("============================================================");
    println!("RawView   链: {} 节点 (容器: {})", raw_chain.len(), raw_container_count);
    println!("ControlView 链: {} 节点 (容器: {})", ctrl_chain.len(), ctrl_container_count);
    println!("ContentView 链: {} 节点 (容器: {})", content_chain.len(), content_container_count);
    println!();

    // 三方对比
    let ctrl_vs_raw_same = ctrl_chain.len() == raw_chain.len()
        && (0..ctrl_chain.len()).all(|i| auto.compare_elements(&ctrl_chain[i], &raw_chain[i]).unwrap_or(false));
    let ctrl_vs_content_same = ctrl_chain.len() == content_chain.len()
        && (0..ctrl_chain.len()).all(|i| auto.compare_elements(&ctrl_chain[i], &content_chain[i]).unwrap_or(false));

    if ctrl_vs_raw_same {
        println!("⚠️  ControlView 和 RawView 链完全一致！");
        if ctrl_vs_content_same {
            println!("⚠️  ControlView 和 ContentView 链也完全一致！");
            println!();
            println!("结论：该应用 (Qt/微信) 将所有节点都标记为 isControl=true 且 isContent=true。");
            println!("所有三种 Walker 看到的是完全相同的树。");
            println!("XPath 精简只能靠 included 过滤 + generate_simplified_elements。");
        } else {
            println!("✅ ControlView 和 ContentView 链不同！");
            println!("   ContentView 过滤了 {} 个节点", (ctrl_chain.len() as i32 - content_chain.len() as i32).abs());
            println!();
            if content_chain.len() < ctrl_chain.len() {
                println!("ContentView 更短（{} vs {}），它过滤掉了非 Content 元素。", content_chain.len(), ctrl_chain.len());
                println!("但 ControlView 没有过滤任何东西 — 所有节点都是 ControlElement。");
                println!("uiautomation-rs 封装层没有问题，ControlViewWalker 正常工作。");
            }
        }
    } else {
        println!("✅ ControlView 和 RawView 链不同！");
        let diff = raw_chain.len() as i32 - ctrl_chain.len() as i32;
        if diff > 0 {
            println!("   ControlView 比 RawView 少 {} 个节点", diff);
        } else {
            println!("   ControlView 比 RawView 多 {} 个节点 (异常)", -diff);
        }
    }
    println!();

    // 7. 验证 included 过滤效果
    println!();
    println!("========== included 过滤效果模拟 ==========");
    println!("（模拟当前代码的 included 逻辑：只保留有 AutomationId/Name 的节点 + 目标节点）");
    println!();

    // 模拟 Normal 捕获的 included 逻辑
    println!("ControlView 链 -> included 过滤后:");
    let last_idx = ctrl_chain.len().saturating_sub(1);
    let mut kept = 0;
    for (i, node) in ctrl_chain.iter().enumerate() {
        let name = node.get_name().unwrap_or_default();
        let aid = node.get_automation_id().unwrap_or_default();
        let ct = node.get_control_type_raw().map(|t| control_type_name(t as u32)).unwrap_or_default();
        let is_target = i == last_idx;
        let included = i != 0 && (is_target || !aid.is_empty() || !name.is_empty());
        if included {
            kept += 1;
            println!("  ✓ 保留: ctrl='{}' name='{}' aid='{}' (target={})",
                ct, name, aid, is_target);
        } else {
            println!("  ✗ 跳过: ctrl='{}' name='{}' aid='{}' (window={})",
                ct, name, aid, i == 0);
        }
    }
    println!("  => 最终 XPath 将包含 {} 个元素节点", kept);
    println!();

    println!("============================================================");
    println!("  测试完成");
    println!("============================================================");

    Ok(())
}
