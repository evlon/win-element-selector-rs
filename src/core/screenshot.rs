// src/core/screenshot.rs
//
// 屏幕截图功能 - 使用 Windows GDI API
// 支持全屏框选截图和元素截图
//
// NOTE: 完整实现需要 image crate 支持，当前为简化版本

use anyhow::Result;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 截取指定屏幕区域
/// 
/// # Arguments
/// * `x` - 左上角 X 坐标
/// * `y` - 左上角 Y 坐标
/// * `width` - 宽度
/// * `height` - 高度
/// 
/// # Returns
/// PNG 格式的字节数据
/// 
/// # Note
/// 当前实现返回空数据，需要添加 image crate 依赖后完善
pub fn capture_region(_x: i32, _y: i32, _width: i32, _height: i32) -> Result<Vec<u8>> {
    // TODO: 实现完整的截图功能
    // 需要添加以下依赖到 Cargo.toml:
    // image = "0.25"
    // dirs = "5.0"
    
    log::warn!("Screenshot capture not yet implemented - requires image crate");
    Ok(vec![])
}

/// 保存截图到文件
pub fn save_screenshot(data: &[u8], path: &str) -> Result<()> {
    if data.is_empty() {
        anyhow::bail!("Screenshot data is empty");
    }
    std::fs::write(path, data)?;
    Ok(())
}

/// 生成截图文件名（带时间戳）
pub fn generate_screenshot_filename() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("uiauto-screenshot-{}.png", timestamp)
}

/// 确保截图目录存在
pub fn ensure_screenshot_directory(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}

/// 获取默认截图目录
pub fn get_default_screenshot_dir() -> PathBuf {
    // 使用当前工作目录下的 screenshots 文件夹
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("screenshots")
}

/// 验证矩形尺寸是否有效
pub fn is_valid_rect(_x: i32, _y: i32, width: i32, height: i32) -> bool {
    width > 0 && height > 0
}

/// 归一化矩形（处理反向拖拽）
pub fn normalize_rect(start: (i32, i32), end: (i32, i32)) -> (i32, i32, i32, i32) {
    let x = start.0.min(end.0);
    let y = start.1.min(end.1);
    let width = (end.0 - start.0).abs();
    let height = (end.1 - start.1).abs();
    (x, y, width, height)
}

/// 将矩形限制在屏幕范围内
pub fn clamp_rect_to_screen(x: i32, y: i32, width: i32, height: i32, screen_width: i32, screen_height: i32) -> (i32, i32, i32, i32) {
    // 首先限制左上角坐标
    let x = x.max(0).min(screen_width);
    let y = y.max(0).min(screen_height);
    
    // 然后限制宽高，确保不超出屏幕边界
    let width = width.min(screen_width - x);
    let height = height.min(screen_height - y);
    
    // 确保宽高不为负
    let width = width.max(0);
    let height = height.max(0);
    
    (x, y, width, height)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_validate_rect_dimensions() {
        assert!(is_valid_rect(0, 0, 100, 100));
        assert!(!is_valid_rect(0, 0, 0, 100));   // 宽度为 0
        assert!(!is_valid_rect(0, 0, 100, 0));   // 高度为 0
        assert!(!is_valid_rect(0, 0, -10, 100)); // 负宽度
    }
    
    #[test]
    fn test_generate_screenshot_filename() {
        let filename = generate_screenshot_filename();
        assert!(filename.starts_with("uiauto-screenshot-"));
        assert!(filename.ends_with(".png"));
    }
    
    #[test]
    fn test_ensure_screenshot_directory() {
        let temp_dir = std::env::temp_dir().join("uiauto-test-screenshots");
        ensure_screenshot_directory(&temp_dir).unwrap();
        assert!(temp_dir.exists());
        // 清理
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    
    #[test]
    fn test_normalize_selection_rect() {
        // 测试矩形归一化（处理反向拖拽）
        let (x, y, w, h) = normalize_rect((100, 100), (50, 50));
        assert_eq!(x, 50);
        assert_eq!(y, 50);
        assert_eq!(w, 50);
        assert_eq!(h, 50);
    }
    
    #[test]
    fn test_normalize_rect_forward() {
        // 正常方向拖拽
        let (x, y, w, h) = normalize_rect((50, 50), (100, 100));
        assert_eq!(x, 50);
        assert_eq!(y, 50);
        assert_eq!(w, 50);
        assert_eq!(h, 50);
    }
    
    #[test]
    fn test_clamp_rect_to_screen() {
        // 测试矩形边界限制
        let (x, y, w, h) = clamp_rect_to_screen(-10, -10, 100, 100, 1920, 1080);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        assert_eq!(w, 100);
        assert_eq!(h, 100);
    }
    
    #[test]
    fn test_clamp_rect_overflow() {
        // 测试超出屏幕的情况
        let (x, y, w, h) = clamp_rect_to_screen(1900, 1000, 100, 100, 1920, 1080);
        assert_eq!(x, 1900);
        assert_eq!(y, 1000);
        assert_eq!(w, 20);  // 被裁剪
        assert_eq!(h, 80);  // 被裁剪
    }
}
