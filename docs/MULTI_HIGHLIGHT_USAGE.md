# 多元素高亮使用指南

## 📋 概述

`MultiHighlightManager` 支持同时显示多个高亮框，适用于以下场景：
- 批量捕获相似元素时，同时高亮所有匹配的元素
- 校验 XPath 时，高亮所有匹配结果
- 对比多个元素的属性时，同时高亮它们

## 🚀 基本用法

### 1. 创建管理器

```rust
use element_selector::gui::multi_highlight::MultiHighlightManager;

let mut manager = MultiHighlightManager::new();
```

### 2. 添加高亮框

```rust
use element_selector::core::model::ElementRect;

// 添加第一个高亮框
let rect1 = ElementRect { x: 100, y: 200, width: 50, height: 30 };
manager.add("element_1", &rect1, "按钮");

// 添加第二个高亮框
let rect2 = ElementRect { x: 200, y: 200, width: 50, height: 30 };
manager.add("element_2", &rect2, "输入框");

// 添加第三个高亮框
let rect3 = ElementRect { x: 300, y: 200, width: 50, height: 30 };
manager.add("element_3", &rect3, "标签");
```

### 3. 更新高亮框位置

```rust
let new_rect = ElementRect { x: 150, y: 250, width: 60, height: 35 };
manager.update("element_1", &new_rect, "新按钮");
```

### 4. 移除单个高亮框

```rust
manager.remove("element_2");
```

### 5. 清除所有高亮框

```rust
manager.clear();
```

### 6. 自动清理（Drop）

```rust
{
    let mut manager = MultiHighlightManager::new();
    manager.add("temp", &rect, "临时");
    // ... 使用
} // manager 离开作用域，自动清理所有窗口
```

## 💡 实际应用场景

### 场景 1：批量捕获相似元素

```rust
fn highlight_similar_elements(samples: &[SimilarElementSample]) {
    let mut manager = MultiHighlightManager::new();
    
    for (i, sample) in samples.iter().enumerate() {
        let id = format!("sample_{}", i);
        manager.add(&id, &sample.rect, &sample.control_type);
    }
    
    // 保持高亮 3 秒后自动清理
    std::thread::sleep(std::time::Duration::from_secs(3));
    manager.clear();
}
```

### 场景 2：XPath 校验结果高亮

```rust
fn highlight_validation_results(elements: &[ElementInfo]) {
    let mut manager = MultiHighlightManager::new();
    
    for (i, elem) in elements.iter().enumerate() {
        let id = format!("match_{}", i);
        let label = format!("{} [{}]", elem.control_type, i + 1);
        manager.add(&id, &elem.rect, &label);
    }
    
    log::info!("高亮了 {} 个匹配元素", elements.len());
    
    // 用户确认后清理
    // manager.clear();
}
```

### 场景 3：在 GUI 应用中使用

```rust
impl SelectorApp {
    fn show_multiple_highlights(&mut self, rects: Vec<(ElementRect, String)>) {
        // 创建或复用管理器
        if self.multi_highlight_manager.is_none() {
            self.multi_highlight_manager = Some(MultiHighlightManager::new());
        }
        
        let manager = self.multi_highlight_manager.as_mut().unwrap();
        
        // 清除旧的高亮
        manager.clear();
        
        // 添加新的高亮
        for (i, (rect, label)) in rects.into_iter().enumerate() {
            manager.add(&format!("item_{}", i), &rect, &label);
        }
    }
}
```

## ⚠️ 注意事项

### 1. 唯一 ID

每个高亮框必须有唯一的 ID，建议使用：
- 索引：`"element_0"`, `"element_1"`
- UUID：`format!("elem_{}", uuid::Uuid::new_v4())`
- 业务标识：`format!("button_{}", button_id)`

### 2. 资源管理

- 每个高亮框会创建 2 个窗口（边框 + 标签）
- 大量高亮框会占用系统资源
- 建议限制同时显示的数量（如最多 50 个）

### 3. 性能考虑

- 创建/销毁窗口是相对耗时的操作
- 频繁更新同一高亮框时，使用 `update()` 而不是 `remove()` + `add()`
- 不需要时及时调用 `clear()` 释放资源

### 4. 线程安全

`MultiHighlightManager` **不是线程安全的**，必须在同一个线程中创建和使用。

如果需要跨线程使用：
```rust
use std::sync::Mutex;

let manager = Arc::new(Mutex::new(MultiHighlightManager::new()));
```

## 🔧 与现有单元素高亮的对比

| 特性 | highlight.rs (单元素) | multi_highlight.rs (多元素) |
|------|---------------------|---------------------------|
| 同时显示数量 | 1 个 | 任意数量 |
| 全局状态 | 是（静态变量） | 否（实例化） |
| 线程安全 | 部分 | 需要外部同步 |
| 资源管理 | 自动覆盖 | 手动管理 |
| 适用场景 | 悬停预览、单次捕获 | 批量操作、对比显示 |

## 📝 未来扩展方向

1. **不同颜色**：支持自定义高亮颜色（红色=错误，绿色=成功，黄色=警告）
2. **动画效果**：闪烁、渐变、脉冲等动画
3. **持久化显示**：某些高亮框可以长期显示，直到用户手动关闭
4. **交互支持**：点击高亮框触发回调
5. **分组管理**：将高亮框分组，批量显示/隐藏

## 🎯 推荐实践

```rust
// ✅ 好的做法：限制数量
if manager.count() >= 50 {
    log::warn!("高亮框数量过多，清除旧的");
    manager.clear();
}

// ✅ 好的做法：使用描述性 ID
manager.add(&format!("button_submit_{}", index), &rect, "提交按钮");

// ❌ 不好的做法：使用随机 ID
manager.add(&format!("{}", random()), &rect, "按钮"); // 难以追踪

// ✅ 好的做法：及时清理
fn temporary_highlight() {
    let mut manager = MultiHighlightManager::new();
    manager.add("temp", &rect, "临时");
    // ... 使用
    manager.clear(); // 显式清理
}
```
