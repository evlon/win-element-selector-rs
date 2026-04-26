// Allow non-upper-case globals for UIA constants from windows crate.
#![allow(non_upper_case_globals)]

// src/capture.rs
//
// Windows UI Automation capture via IUIAutomation COM interface.
// Non-Windows platforms compile with a rich mock for UI development.

use crate::model::{CaptureResult, ElementRect, HierarchyNode, ValidationResult};
use log::{debug, error};

// ═══════════════════════════════════════════════════════════════════════════════
// Windows implementation
// ═══════════════════════════════════════════════════════════════════════════════
#[cfg(target_os = "windows")]
pub mod uia {
    use super::*;
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::POINT,
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationElement,
                    IUIAutomationTreeWalker,
                },
                WindowsAndMessaging::GetCursorPos,
            },
        },
    };

    // Lazily created IUIAutomation instance (COM STA — must stay on UI thread).
    thread_local! {
        static AUTOMATION: std::cell::RefCell<Option<IUIAutomation>> =
            std::cell::RefCell::new(None);
    }

    fn get_automation() -> anyhow::Result<IUIAutomation> {
        AUTOMATION.with(|cell| {
            let mut opt = cell.borrow_mut();
            if opt.is_none() {
                let auto: IUIAutomation = unsafe {
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                }
                .map_err(|e| anyhow::anyhow!("CoCreateInstance IUIAutomation: {e}"))?;
                *opt = Some(auto);
            }
            Ok(opt.as_ref().unwrap().clone())
        })
    }

    /// Capture the element under the mouse cursor.
    #[allow(dead_code)]
    pub fn capture_at_cursor() -> CaptureResult {
        let pt = unsafe {
            let mut p = POINT::default();
            if GetCursorPos(&mut p).is_err() {
                return CaptureResult {
                    hierarchy: vec![],
                    cursor_x: 0, cursor_y: 0,
                    error: Some("GetCursorPos 失败".to_string()),
                };
            }
            p
        };
        capture_at_point(pt.x, pt.y)
    }

    /// Capture the element at a specific screen coordinate.
    pub fn capture_at_point(x: i32, y: i32) -> CaptureResult {
        match do_capture(x, y) {
            Ok(result) => result,
            Err(e) => {
                error!("capture_at_point({x},{y}) failed: {e}");
                CaptureResult {
                    hierarchy: vec![],
                    cursor_x: x, cursor_y: y,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    fn do_capture(x: i32, y: i32) -> anyhow::Result<CaptureResult> {
        let auto = get_automation()?;
        let pt   = POINT { x, y };

        let target: IUIAutomationElement = unsafe {
            auto.ElementFromPoint(pt)
                .map_err(|e| anyhow::anyhow!("ElementFromPoint: {e}"))?
        };

        let mut chain: Vec<IUIAutomationElement> = vec![target.clone()];

        // Walk up ancestors (max 32 levels).
        let walker: IUIAutomationTreeWalker = unsafe {
            auto.ControlViewWalker()
                .map_err(|e| anyhow::anyhow!("ControlViewWalker: {e}"))?
        };

        let mut current = target.clone();
        for _ in 0..32 {
            let parent = unsafe { walker.GetParentElement(&current) };
            match parent {
                Ok(p) => {
                    // Null element means we've reached the root.
                    let is_null = unsafe { 
                        auto.CompareElements(&p, &auto.GetRootElement()?)
                            .unwrap_or(windows::Win32::Foundation::BOOL(0))
                            .as_bool()
                    };
                    if is_null {
                        chain.push(p);
                        break;
                    }
                    chain.push(p.clone());
                    current = p;
                }
                Err(_) => break,
            }
        }

        chain.reverse(); // root → target

        let mut hierarchy: Vec<HierarchyNode> = Vec::with_capacity(chain.len());
        for elem in &chain {
            if let Some(node) = element_to_node(elem, &auto) {
                hierarchy.push(node);
            }
        }

        // Compute sibling index for the last element (target).
        if let Some(last) = hierarchy.last_mut() {
            last.index = sibling_index(&target, &walker).unwrap_or(0);
            if last.index > 0 {
                // Update the Index filter value.
                if let Some(f) = last.filters.iter_mut().find(|f| f.name == "Index") {
                    f.value   = last.index.to_string();
                    f.enabled = true;
                }
            }
        }

        Ok(CaptureResult { hierarchy, cursor_x: x, cursor_y: y, error: None })
    }

    fn element_to_node(
        elem: &IUIAutomationElement,
        _auto: &IUIAutomation,
    ) -> Option<HierarchyNode> {
        let control_type = unsafe {
            elem.CurrentControlType()
                .map(control_type_name)
                .unwrap_or_default()
        };

        let automation_id = get_bstr(unsafe { elem.CurrentAutomationId() });
        let class_name    = get_bstr(unsafe { elem.CurrentClassName() });
        let name          = get_bstr(unsafe { elem.CurrentName() });
        let process_id    = unsafe { elem.CurrentProcessId().unwrap_or(0) as u32 };

        let rect = unsafe {
            elem.CurrentBoundingRectangle()
                .map(|r| ElementRect {
                    x:      r.left,
                    y:      r.top,
                    width:  r.right  - r.left,
                    height: r.bottom - r.top,
                })
                .unwrap_or_default()
        };

        debug!(
            "element: type={control_type} aid={automation_id} \
             class={class_name} name={name} pid={process_id}"
        );

        Some(HierarchyNode::new(
            control_type, automation_id, class_name, name,
            0, rect, process_id,
        ))
    }

    /// 1-based index among same-type siblings under the parent.
    fn sibling_index(
        target: &IUIAutomationElement,
        walker: &IUIAutomationTreeWalker,
    ) -> Option<i32> {
        let parent = unsafe { walker.GetParentElement(target).ok()? };
        let mut child = unsafe { walker.GetFirstChildElement(&parent).ok()? };
        let target_ct = unsafe { target.CurrentControlType().ok()? };

        let mut idx = 0i32;
        loop {
            let ct = unsafe { child.CurrentControlType().ok()? };
            if ct == target_ct {
                idx += 1;
            }
            // Compare by RuntimeId.
            let same = unsafe {
                let aid_child  = child.CurrentAutomationId().unwrap_or_default();
                let aid_target = target.CurrentAutomationId().unwrap_or_default();
                aid_child == aid_target
            };
            if same { return Some(idx); }
            match unsafe { walker.GetNextSiblingElement(&child) } {
                Ok(next) => child = next,
                Err(_)   => break,
            }
        }
        None
    }

    fn get_bstr<T: Into<BSTR>>(r: windows::core::Result<T>) -> String {
        r.ok()
            .map(|b| {
                let bstr: BSTR = b.into();
                bstr.to_string()
            })
            .unwrap_or_default()
    }

    fn control_type_name(id: windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID) -> String {
        use windows::Win32::UI::Accessibility::*;
        match id {
            UIA_ButtonControlTypeId       => "Button",
            UIA_CalendarControlTypeId     => "Calendar",
            UIA_CheckBoxControlTypeId     => "CheckBox",
            UIA_ComboBoxControlTypeId     => "ComboBox",
            UIA_CustomControlTypeId       => "Custom",
            UIA_DataGridControlTypeId     => "DataGrid",
            UIA_DataItemControlTypeId     => "DataItem",
            UIA_DocumentControlTypeId     => "Document",
            UIA_EditControlTypeId         => "Edit",
            UIA_GroupControlTypeId        => "Group",
            UIA_HeaderControlTypeId       => "Header",
            UIA_HeaderItemControlTypeId   => "HeaderItem",
            UIA_HyperlinkControlTypeId    => "Hyperlink",
            UIA_ImageControlTypeId        => "Image",
            UIA_ListControlTypeId         => "List",
            UIA_ListItemControlTypeId     => "ListItem",
            UIA_MenuBarControlTypeId      => "MenuBar",
            UIA_MenuControlTypeId         => "Menu",
            UIA_MenuItemControlTypeId     => "MenuItem",
            UIA_PaneControlTypeId         => "Pane",
            UIA_ProgressBarControlTypeId  => "ProgressBar",
            UIA_RadioButtonControlTypeId  => "RadioButton",
            UIA_ScrollBarControlTypeId    => "ScrollBar",
            UIA_SemanticZoomControlTypeId => "SemanticZoom",
            UIA_SeparatorControlTypeId    => "Separator",
            UIA_SliderControlTypeId       => "Slider",
            UIA_SpinnerControlTypeId      => "Spinner",
            UIA_SplitButtonControlTypeId  => "SplitButton",
            UIA_StatusBarControlTypeId    => "StatusBar",
            UIA_TabControlTypeId          => "Tab",
            UIA_TabItemControlTypeId      => "TabItem",
            UIA_TableControlTypeId        => "Table",
            UIA_TextControlTypeId         => "Text",
            UIA_ThumbControlTypeId        => "Thumb",
            UIA_TitleBarControlTypeId     => "TitleBar",
            UIA_ToolBarControlTypeId      => "ToolBar",
            UIA_ToolTipControlTypeId      => "ToolTip",
            UIA_TreeControlTypeId         => "Tree",
            UIA_TreeItemControlTypeId     => "TreeItem",
            UIA_WindowControlTypeId       => "Window",
            _                             => "Control",
        }.to_string()
    }

    // ── Validation ────────────────────────────────────────────────────────────

    /// Find all elements matching the given XPath-style expression.
    /// We implement a subset: `//ControlType[@Attr='val' and ...]` chains.
    pub fn validate_xpath(xpath: &str) -> ValidationResult {
        let auto = match get_automation() {
            Ok(a)  => a,
            Err(e) => return ValidationResult::Error(e.to_string()),
        };

        let root = match unsafe { auto.GetRootElement() } {
            Ok(r)  => r,
            Err(e) => return ValidationResult::Error(format!("GetRootElement: {e}")),
        };

        match find_by_xpath(&auto, &root, xpath) {
            Ok(results) if results.is_empty() => ValidationResult::NotFound,
            Ok(results) => {
                let first_rect = results.first().and_then(|e| {
                    unsafe { e.CurrentBoundingRectangle().ok() }.map(|r| ElementRect {
                        x: r.left, y: r.top,
                        width: r.right - r.left, height: r.bottom - r.top,
                    })
                });
                ValidationResult::Found { count: results.len(), first_rect }
            }
            Err(e) => ValidationResult::Error(e.to_string()),
        }
    }

    fn find_by_xpath(
        auto: &IUIAutomation,
        _root: &IUIAutomationElement,
        xpath: &str,
    ) -> anyhow::Result<Vec<IUIAutomationElement>> {
        use windows::Win32::UI::Accessibility::*;

        // Parse segments split by "//".
        let segments: Vec<&str> = xpath
            .split("//")
            .filter(|s| !s.is_empty())
            .collect();

        if segments.is_empty() {
            return Ok(vec![]);
        }

        let root = unsafe { auto.GetRootElement()? };
        let mut candidates: Vec<IUIAutomationElement> = vec![root];

        for seg in &segments {
            let (tag, preds) = parse_segment(seg);
            let mut next_candidates: Vec<IUIAutomationElement> = Vec::new();

            for parent in &candidates {
                let condition: IUIAutomationCondition = unsafe {
                    auto.CreateTrueCondition()?
                };
                let found = unsafe {
                    parent.FindAll(windows::Win32::UI::Accessibility::TreeScope_Subtree, &condition)?
                };

                let count = unsafe { found.Length()? };
                for i in 0..count {
                    let elem = unsafe { found.GetElement(i)? };
                    let ct = unsafe {
                        elem.CurrentControlType()
                            .map(control_type_name)
                            .unwrap_or_default()
                    };
                    if !tag.is_empty() && tag != "*" && ct != tag {
                        continue;
                    }
                    // Check predicates.
                    if preds.iter().all(|(attr, op, val)| {
                        let actual = match attr.as_str() {
                            "AutomationId" => get_bstr(unsafe { elem.CurrentAutomationId() }),
                            "ClassName"    => get_bstr(unsafe { elem.CurrentClassName() }),
                            "Name"         => get_bstr(unsafe { elem.CurrentName() }),
                            _              => String::new(),
                        };
                        check_predicate(&actual, op, val)
                    }) {
                        next_candidates.push(elem);
                    }
                }
            }
            candidates = next_candidates;
            if candidates.is_empty() { break; }
        }

        Ok(candidates)
    }

    fn parse_segment(seg: &str) -> (String, Vec<(String, String, String)>) {
        if let Some(bracket) = seg.find('[') {
            let tag   = seg[..bracket].trim().to_string();
            let inner = seg[bracket + 1..seg.rfind(']').unwrap_or(seg.len())].trim();
            let preds = parse_predicates(inner);
            (tag, preds)
        } else {
            (seg.trim().to_string(), vec![])
        }
    }

    /// Very lightweight predicate parser for `@Attr op 'val'` and `contains(...)`.
    fn parse_predicates(s: &str) -> Vec<(String, String, String)> {
        let mut result = Vec::new();
        for part in s.split(" and ") {
            let part = part.trim();
            if part.starts_with("contains(") {
                // contains(@Attr, 'val')
                let inner = &part[9..part.rfind(')').unwrap_or(part.len())];
                let mut parts = inner.splitn(2, ',');
                let attr = parts.next().unwrap_or("").trim().trim_start_matches('@').to_string();
                let val  = parts.next().unwrap_or("").trim().trim_matches('\'').to_string();
                result.push((attr, "contains".to_string(), val));
            } else if part.starts_with("starts-with(") {
                let inner = &part[12..part.rfind(')').unwrap_or(part.len())];
                let mut parts = inner.splitn(2, ',');
                let attr = parts.next().unwrap_or("").trim().trim_start_matches('@').to_string();
                let val  = parts.next().unwrap_or("").trim().trim_matches('\'').to_string();
                result.push((attr, "starts-with".to_string(), val));
            } else if let Some(eq_pos) = part.find("!=") {
                let attr = part[..eq_pos].trim().trim_start_matches('@').to_string();
                let val  = part[eq_pos + 2..].trim().trim_matches('\'').to_string();
                result.push((attr, "!=".to_string(), val));
            } else if let Some(eq_pos) = part.find('=') {
                let attr = part[..eq_pos].trim().trim_start_matches('@').to_string();
                let val  = part[eq_pos + 1..].trim().trim_matches('\'').to_string();
                result.push((attr, "=".to_string(), val));
            }
        }
        result
    }

    fn check_predicate(actual: &str, op: &str, expected: &str) -> bool {
        match op {
            "="           => actual == expected,
            "!="          => actual != expected,
            "contains"    => actual.contains(expected),
            "starts-with" => actual.starts_with(expected),
            _             => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Windows mock (rich demo data)
// ═══════════════════════════════════════════════════════════════════════════════
#[cfg(not(target_os = "windows"))]
pub mod uia {
    use super::*;

    pub fn capture_at_cursor() -> CaptureResult { mock() }
    pub fn capture_at_point(_x: i32, _y: i32) -> CaptureResult { mock() }

    pub fn validate_xpath(xpath: &str) -> ValidationResult {
        if xpath.trim().is_empty() {
            ValidationResult::Error("XPath 为空".into())
        } else {
            ValidationResult::Found {
                count: 1,
                first_rect: Some(ElementRect { x: 200, y: 300, width: 120, height: 30 }),
            }
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn capture() -> CaptureResult {
    uia::capture_at_cursor()
}

#[allow(dead_code)]
pub fn capture_at(x: i32, y: i32) -> CaptureResult {
    uia::capture_at_point(x, y)
}

pub fn validate(xpath: &str) -> ValidationResult {
    uia::validate_xpath(xpath)
}

// ─── Rich mock data ──────────────────────────────────────────────────────────

pub fn mock() -> CaptureResult {
    CaptureResult {
        hierarchy: vec![
            HierarchyNode::new(
                "Window", "MainAppWindow", "WpfWindow", "My Application  —  文档1",
                0, ElementRect { x: 0, y: 0, width: 1280, height: 800 }, 12345,
            ),
            HierarchyNode::new(
                "Pane", "", "DockPanel", "", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 770 }, 12345,
            ),
            HierarchyNode::new(
                "ToolBar", "mainToolbar", "ToolBarTray", "主工具栏", 0,
                ElementRect { x: 0, y: 30, width: 1280, height: 36 }, 12345,
            ),
            HierarchyNode::new(
                "Button", "btnSave", "Button", "保存", 2,
                ElementRect { x: 120, y: 34, width: 80, height: 28 }, 12345,
            ),
        ],
        cursor_x: 160,
        cursor_y: 48,
        error: None,
    }
}
