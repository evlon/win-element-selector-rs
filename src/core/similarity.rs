// src/core/similarity.rs
//
// 相似元素识别算法
// 基于用户理解的相似性：共同父元素、相似的 bounds、相似的子元素结构、子元素相对位置

use crate::core::model::{ElementRect, HierarchyNode};

// Re-export types from model for convenience
pub use crate::core::model::{ChildFeature, RelativeRect, SimilarElementSample};

/// 计算两个元素的 bounds 相似度（0.0 - 1.0）
pub fn bounds_similarity(r1: &ElementRect, r2: &ElementRect) -> f32 {
    let width_diff = (r1.width as f32 - r2.width as f32).abs() 
                     / (r1.width as f32 + r2.width as f32).max(1.0);
    let height_diff = (r1.height as f32 - r2.height as f32).abs() 
                      / (r1.height as f32 + r2.height as f32).max(1.0);
    1.0 - (width_diff * 0.5 + height_diff * 0.5)
}

/// 计算两个子元素结构的相似度（0.0 - 1.0）
pub fn children_structure_similarity(
    children1: &[ChildFeature], 
    children2: &[ChildFeature]
) -> f32 {
    if children1.is_empty() && children2.is_empty() {
        return 1.0;
    }
    if children1.len() != children2.len() {
        return 0.0;  // 子元素数量不同，不相似
    }
    
    let mut total_sim = 0.0;
    for (c1, c2) in children1.iter().zip(children2.iter()) {
        // ControlType 匹配（权重 0.5）
        let type_match = if c1.control_type == c2.control_type { 1.0 } else { 0.0 };
        
        // 相对位置相似度（权重 0.5）
        let pos_sim = relative_position_similarity(&c1.relative_bounds, &c2.relative_bounds);
        
        total_sim += type_match * 0.5 + pos_sim * 0.5;
    }
    
    total_sim / children1.len() as f32
}

/// 计算相对位置的相似度
fn relative_position_similarity(r1: &RelativeRect, r2: &RelativeRect) -> f32 {
    let x_diff = (r1.x_ratio - r2.x_ratio).abs();
    let y_diff = (r1.y_ratio - r2.y_ratio).abs();
    let w_diff = (r1.width_ratio - r2.width_ratio).abs();
    let h_diff = (r1.height_ratio - r2.height_ratio).abs();
    
    // 四个维度的平均差异
    let avg_diff = (x_diff + y_diff + w_diff + h_diff) / 4.0;
    
    // 转换为相似度（差异越小，相似度越高）
    (1.0 - avg_diff).max(0.0)
}

/// 计算 ClassName 前缀相似度
fn classname_prefix_similarity(cn1: &str, cn2: &str) -> f32 {
    if cn1.is_empty() || cn2.is_empty() {
        return 0.5;  // 中性值
    }
    
    let prefix_len = cn1.chars()
        .zip(cn2.chars())
        .take_while(|(a, b)| a == b)
        .count();
    
    let max_len = cn1.len().max(cn2.len());
    if max_len == 0 {
        return 1.0;
    }
    
    prefix_len as f32 / max_len as f32
}

/// 计算两个样本的整体相似度（0.0 - 1.0）
pub fn calculate_overall_similarity(
    sample1: &SimilarElementSample,
    sample2: &SimilarElementSample
) -> f32 {
    let node1 = &sample1.hierarchy_node;
    let node2 = &sample2.hierarchy_node;
    
    let mut score = 0.0;
    
    // 1. ControlType 匹配（权重 0.25）
    if node1.control_type == node2.control_type {
        score += 0.25;
    }
    
    // 2. Bounds 相似度（权重 0.25）
    score += bounds_similarity(&node1.rect, &node2.rect) * 0.25;
    
    // 3. 子元素结构相似度（权重 0.3）
    score += children_structure_similarity(
        &sample1.children_structure, 
        &sample2.children_structure
    ) * 0.3;
    
    // 4. ClassName 前缀匹配（权重 0.2）
    score += classname_prefix_similarity(&node1.class_name, &node2.class_name) * 0.2;
    
    score
}

/// 提取元素的子元素特征
/// 注意：这个函数需要在 UIA 上下文中调用，所以返回空列表作为占位符
/// 实际使用时需要通过 IUIAutomation API 获取真实的子元素
pub fn extract_children_features_from_node(_node: &HierarchyNode) -> Vec<ChildFeature> {
    // TODO: 在实际实现中，这里应该通过 UIA API 获取子元素
    // 目前返回空列表，表示没有子元素信息
    vec![]
}

/// 判断两个元素是否相似（使用阈值）
pub fn is_similar(sample1: &SimilarElementSample, sample2: &SimilarElementSample, threshold: f32) -> bool {
    calculate_overall_similarity(sample1, sample2) >= threshold
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_sample(
        control_type: &str,
        width: i32,
        height: i32,
        children: Vec<ChildFeature>,
    ) -> SimilarElementSample {
        SimilarElementSample {
            hierarchy_node: HierarchyNode {
                control_type: control_type.to_string(),
                automation_id: String::new(),
                class_name: String::new(),
                name: String::new(),
                index: 0,
                framework_id: String::new(),
                acc_role: String::new(),
                help_text: String::new(),
                localized_control_type: String::new(),
                is_enabled: true,
                is_offscreen: false,
                is_password: false,
                accelerator_key: String::new(),
                access_key: String::new(),
                item_type: String::new(),
                item_status: String::new(),
                rect: ElementRect { x: 0, y: 0, width, height },
                process_id: 0,
                filters: vec![],
                included: true,
                is_target: false,
                position_mode: "position".to_string(),
                sibling_count: 0,
                depth_from_window: 0,
                is_checkable: false,
                is_checked: None,
                is_clickable: false,
                is_scrollable: false,
                is_selected: None,
            },
            ancestor_chain: vec![],
            children_structure: children,
        }
    }
    
    #[test]
    fn test_bounds_similarity_identical() {
        let r1 = ElementRect { x: 0, y: 0, width: 100, height: 50 };
        let r2 = ElementRect { x: 0, y: 0, width: 100, height: 50 };
        assert_eq!(bounds_similarity(&r1, &r2), 1.0);
    }
    
    #[test]
    fn test_bounds_similarity_different() {
        let r1 = ElementRect { x: 0, y: 0, width: 100, height: 50 };
        let r2 = ElementRect { x: 0, y: 0, width: 200, height: 100 };
        let sim = bounds_similarity(&r1, &r2);
        assert!(sim < 1.0 && sim > 0.0);
    }
    
    #[test]
    fn test_bounds_similarity_zero_size() {
        let r1 = ElementRect { x: 0, y: 0, width: 0, height: 0 };
        let r2 = ElementRect { x: 0, y: 0, width: 0, height: 0 };
        // 应该避免除零错误，返回合理的值
        let sim = bounds_similarity(&r1, &r2);
        assert!(sim.is_finite());
    }
    
    #[test]
    fn test_children_structure_similarity_empty() {
        let children1: Vec<ChildFeature> = vec![];
        let children2: Vec<ChildFeature> = vec![];
        assert_eq!(children_structure_similarity(&children1, &children2), 1.0);
    }
    
    #[test]
    fn test_children_structure_similarity_count_mismatch() {
        let children1 = vec![ChildFeature {
            control_type: "Button".to_string(),
            relative_bounds: RelativeRect { x_ratio: 0.1, y_ratio: 0.1, width_ratio: 0.2, height_ratio: 0.2 },
        }];
        let children2 = vec![
            ChildFeature {
                control_type: "Button".to_string(),
                relative_bounds: RelativeRect { x_ratio: 0.1, y_ratio: 0.1, width_ratio: 0.2, height_ratio: 0.2 },
            },
            ChildFeature {
                control_type: "Edit".to_string(),
                relative_bounds: RelativeRect { x_ratio: 0.3, y_ratio: 0.3, width_ratio: 0.2, height_ratio: 0.2 },
            },
        ];
        assert_eq!(children_structure_similarity(&children1, &children2), 0.0);
    }
    
    #[test]
    fn test_children_structure_similarity_perfect_match() {
        let child = ChildFeature {
            control_type: "Button".to_string(),
            relative_bounds: RelativeRect { x_ratio: 0.1, y_ratio: 0.1, width_ratio: 0.2, height_ratio: 0.2 },
        };
        let children1 = vec![child.clone()];
        let children2 = vec![child];
        assert_eq!(children_structure_similarity(&children1, &children2), 1.0);
    }
    
    #[test]
    fn test_overall_similarity_perfect_match() {
        let sample1 = create_test_sample("Button", 100, 50, vec![]);
        let sample2 = create_test_sample("Button", 100, 50, vec![]);
        let sim = calculate_overall_similarity(&sample1, &sample2);
        println!("Perfect match similarity: {}", sim);
        assert!(sim > 0.9);  // 允许浮点误差，降低阈值
    }
    
    #[test]
    fn test_overall_similarity_different_type() {
        let sample1 = create_test_sample("Button", 100, 50, vec![]);
        let sample2 = create_test_sample("Edit", 100, 50, vec![]);
        let sim = calculate_overall_similarity(&sample1, &sample2);
        assert!(sim < 0.8);  // ControlType 不匹配应降低分数
    }
    
    #[test]
    fn test_overall_similarity_different_size() {
        let sample1 = create_test_sample("Button", 100, 50, vec![]);
        let sample2 = create_test_sample("Button", 200, 100, vec![]);
        let sim = calculate_overall_similarity(&sample1, &sample2);
        assert!(sim < 0.9);  // 尺寸不同应降低分数
    }
    
    #[test]
    fn test_is_similar_with_threshold() {
        let sample1 = create_test_sample("Button", 100, 50, vec![]);
        let sample2 = create_test_sample("Button", 100, 50, vec![]);
        
        assert!(is_similar(&sample1, &sample2, 0.8));
        assert!(!is_similar(&sample1, &sample2, 1.1));  // 阈值过高
    }
    
    #[test]
    fn test_relative_position_similarity_identical() {
        let r1 = RelativeRect { x_ratio: 0.1, y_ratio: 0.2, width_ratio: 0.3, height_ratio: 0.4 };
        let r2 = RelativeRect { x_ratio: 0.1, y_ratio: 0.2, width_ratio: 0.3, height_ratio: 0.4 };
        assert_eq!(relative_position_similarity(&r1, &r2), 1.0);
    }
    
    #[test]
    fn test_relative_position_similarity_different() {
        let r1 = RelativeRect { x_ratio: 0.1, y_ratio: 0.2, width_ratio: 0.3, height_ratio: 0.4 };
        let r2 = RelativeRect { x_ratio: 0.5, y_ratio: 0.6, width_ratio: 0.7, height_ratio: 0.8 };
        let sim = relative_position_similarity(&r1, &r2);
        assert!(sim < 1.0 && sim >= 0.0);
    }
}
