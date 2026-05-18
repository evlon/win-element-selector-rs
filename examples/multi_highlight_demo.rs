// examples/multi_highlight_demo.rs
//
// 多元素高亮功能演示
// 
// 注意：此示例仅用于展示 API 用法，实际运行需要在 GUI 环境中
// 因为高亮窗口需要 Windows 消息循环

use std::time::Duration;

fn main() {
    println!("=== 多元素高亮功能演示 ===\n");
    println!("此示例展示了 MultiHighlightManager 的 API 用法");
    println!("由于高亮窗口需要 Windows 消息循环，请在 GUI 应用中使用\n");

    println!("基本用法：");
    println!("1. 创建管理器: let mut manager = MultiHighlightManager::new();");
    println!("2. 添加高亮框: manager.add(\"id\", &rect, \"标签\");");
    println!("3. 更新高亮框: manager.update(\"id\", &new_rect, \"新标签\");");
    println!("4. 移除高亮框: manager.remove(\"id\");");
    println!("5. 清除所有: manager.clear();");
    println!("6. 批量添加: manager.add_multiple(&[(\"id1\", &rect1, \"标签1\"), ...]);");
    
    println!("\n使用场景：");
    println!("- 批量捕获相似元素时，同时高亮所有匹配的元素");
    println!("- 校验 XPath 时，高亮所有匹配结果");
    println!("- 对比多个元素的属性时，同时高亮它们");
    
    println!("\n性能建议：");
    println!("- 少于 20 个高亮框：无压力");
    println!("- 20-50 个高亮框：可接受");
    println!("- 超过 50 个：建议分页显示或限制数量");
    
    println!("\n=== 演示结束 ===");
}
