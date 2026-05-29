// src/api/offset_parser.rs
//
// 偏移表达式解析器
// 支持格式：left+20%, top-10px, right-5%, bottom+15px

use crate::api::types::Rect;
use regex::Regex;

/// 解析偏移表达式
/// 
/// 支持的格式：
/// - 表达式："{reference}{operator}{value}{unit}"
///   例如：left+20%, top-10px, right-5%, bottom+15px
/// 
/// # Arguments
/// * `expr` - 偏移表达式字符串
/// * `rect` - 元素矩形（用于百分比计算）
/// 
/// # Returns
/// 相对于元素左上角的偏移量 (offset_x, offset_y)
pub fn parse_offset_expression(expr: &str, rect: &Rect) -> Result<(f32, f32), String> {
    // 正则匹配：^(left|right|top|bottom)([+-])(\d+(?:\.\d+)?)(%|px)$
    let re = Regex::new(r"^(left|right|top|bottom)([+-])(\d+(?:\.\d+)?)(%|px)$")
        .map_err(|e| format!("Invalid regex: {}", e))?;
    
    if let Some(caps) = re.captures(expr) {
        let reference = caps.get(1).unwrap().as_str();
        let operator = caps.get(2).unwrap().as_str();
        let value: f32 = caps.get(3).unwrap().as_str().parse()
            .map_err(|e| format!("Invalid number: {}", e))?;
        let unit = caps.get(4).unwrap().as_str();
        
        // 计算实际像素值
        let pixel_value = if unit == "%" {
            match reference {
                "left" | "right" => rect.width as f32 * value / 100.0,
                "top" | "bottom" => rect.height as f32 * value / 100.0,
                _ => return Err("Invalid reference".to_string()),
            }
        } else {
            // px 单位
            value
        };
        
        // 计算偏移量（相对于元素左上角）
        // 语义：+/- 表示坐标值的增减
        match reference {
            "left" => {
                // left+X: x = X (距离左边 X)
                // left-X: x = -X (在左边外侧)
                if operator == "+" {
                    Ok((pixel_value, 0.0))
                } else {
                    Ok((-pixel_value, 0.0))
                }
            },
            "right" => {
                // right-X: x = width - X (距离右边 X，向内)
                // right+X: x = width + X (在右边外侧)
                if operator == "-" {
                    Ok((rect.width as f32 - pixel_value, 0.0))
                } else {
                    Ok((rect.width as f32 + pixel_value, 0.0))
                }
            },
            "top" => {
                // top-X: y = -X (在顶部外侧)
                // top+X: y = X (距离顶部 X，向内)
                if operator == "-" {
                    Ok((0.0, -pixel_value))
                } else {
                    Ok((0.0, pixel_value))
                }
            },
            "bottom" => {
                // bottom+X: y = height - X (距离底部 X，向内)
                // bottom-X: y = height + X (在底部外侧)
                if operator == "+" {
                    Ok((0.0, rect.height as f32 - pixel_value))
                } else {
                    Ok((0.0, rect.height as f32 + pixel_value))
                }
            },
            _ => Err("Invalid reference".to_string()),
        }
    } else {
        Err(format!("Invalid offset expression: {}", expr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect { x, y, width, height }
    }

    #[test]
    fn test_parse_left_percentage() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("left+20%", &rect).unwrap();
        // left+20% 应该是距离左边 20% 的位置，即 x = 200 * 0.2 = 40
        assert_eq!(result.0, 40.0);
        assert_eq!(result.1, 0.0);
    }

    #[test]
    fn test_parse_right_percentage() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("right-5%", &rect).unwrap();
        // right-5% 应该是距离右边 5% 的位置，即 x = 200 - (200 * 0.05) = 190
        assert_eq!(result.0, 190.0);
        assert_eq!(result.1, 0.0);
    }

    #[test]
    fn test_parse_top_pixels() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("top-10px", &rect).unwrap();
        // top-10px 应该是距离顶部向上 10px，即 y = -10
        assert_eq!(result.0, 0.0);
        assert_eq!(result.1, -10.0);
    }

    #[test]
    fn test_parse_bottom_pixels() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("bottom+15px", &rect).unwrap();
        // bottom+15px 应该是距离底部向下 15px，即 y = 100 - 15 = 85
        assert_eq!(result.0, 0.0);
        assert_eq!(result.1, 85.0);
    }

    #[test]
    fn test_invalid_expression() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("invalid", &rect);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_reference() {
        let rect = make_rect(100, 100, 200, 100);
        let result = parse_offset_expression("center+10%", &rect);
        assert!(result.is_err());
    }
}
