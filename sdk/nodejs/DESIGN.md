# Node.js SDK 设计文档

> Element Selector Server 的 Node.js SDK，提供拟人化 UI 自动化操作能力

## 一、概述

### 1.1 目标

为 element-selector-server 提供一个易用的 Node.js SDK，核心特性：
- **拟人化操作**：所有鼠标/键盘操作自动模拟人类行为
- **智能空闲移动**：在指定元素区域内持续随机移动鼠标
- **自动冲突处理**：API 调用与空闲移动冲突时服务端自动暂停/恢复
- **人工干预检测**：检测到用户操作时自动暂停，静止后自动恢复

### 1.2 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        Node.js SDK                               │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────┐     │
│  │   Client    │  │ HumanizeCtx  │  │  Type Definitions   │     │
│  └─────────────┘  └──────────────┘  └─────────────────────┘     │
└────────────────────────────┬────────────────────────────────────┘
                             │ HTTP API
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   element-selector-server                        │
│  ┌────────────┐  ┌─────────────┐  ┌──────────────────────────┐  │
│  │  HTTP API  │  │ IdleMotion  │  │  Human Activity Monitor  │  │
│  │  Handlers  │  │   Manager   │  │      (Background)        │  │
│  └────────────┘  └─────────────┘  └──────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Global State (Arc<RwLock<IdleMotionState>>)  │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Windows UI Automation                         │
│            (Mouse Control, Keyboard, Element Finder)             │
└─────────────────────────────────────────────────────────────────┘
```

---

## 二、现有 HTTP API

### 2.1 已实现接口

| 接口 | 方法 | 描述 |
|------|------|------|
| `/api/health` | GET | 健康检查 |
| `/api/window/list` | POST | 列出所有窗口 |
| `/api/element` | GET | 查找元素信息 |
| `/api/mouse/move` | POST | 拟人化移动鼠标 |
| `/api/mouse/click` | POST | 拟人化点击元素 |

### 2.2 现有类型定义

```rust
// 请求类型
struct MouseMoveRequest {
    target: Point,
    options: Option<MouseMoveOptions>,
}

struct MouseClickRequest {
    window: WindowSelector,
    xpath: String,
    options: Option<MouseClickOptions>,
}

struct ElementQuery {
    window_selector: String,
    xpath: String,
    random_range: f32,  // 默认 0.55
}

// 响应类型
struct ElementResponse {
    found: bool,
    element: Option<ElementInfo>,
    error: Option<String>,
}

struct MouseMoveResponse {
    success: bool,
    start_point: Point,
    end_point: Point,
    duration_ms: u64,
    error: Option<String>,
}

struct MouseClickResponse {
    success: bool,
    click_point: Point,
    element: Option<ClickedElement>,
    error: Option<String>,
}
```

---

## 三、新增功能设计

### 3.1 空闲移动 (Idle Motion)

核心功能：在指定元素区域内持续随机移动鼠标，模拟用户"浏览"行为。

#### 3.1.1 状态机设计

```
┌─────────────────────────────────────────────────────────────────────┐
│                      完整状态机（三层控制）                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────┐                                                    │
│  │   停止状态   │ ◄──────────────────────────────────────────┐     │
│  └──────┬──────┘         无操作超时 M 秒                       │     │
│         │ startIdleMotion()                                       │     │
│         ↓                                                     │     │
│  ┌─────────────┐    检测到用户操作     ┌─────────────┐       │     │
│  │  空闲移动中  │ ────────────────────→ │   暂停中    │       │     │
│  └──────┬──────┘                        └──────┬──────┘       │     │
│         │                                      │               │     │
│         │                                      │ 用户静止 N 秒  │     │
│         │                                      ↓               │     │
│         │                               ┌─────────────┐       │     │
│         │                               │   恢复移动   │ ──────┘     │
│         │                               └─────────────┘             │
│         │                                                           │
│         │ API 调用                                                  │     │
│         │ (自动暂停→执行→恢复)                                        │     │
│         └──────────────────────────────────────────────────────────│
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

#### 3.1.2 暂停原因枚举

```rust
pub enum PauseReason {
    ApiCall,           // API 调用自动暂停
    HumanMouseMove,    // 检测到人工鼠标移动
    HumanKeyboard,     // 检测到人工键盘操作
    Manual,            // 手动调用暂停
}
```

#### 3.1.3 自动暂停/恢复机制

**核心设计原则**：服务端自动处理，客户端无感知

```
API 请求进入
     │
     ▼
┌─────────────────┐
│ 检查空闲移动状态 │
└────────┬────────┘
         │
    ┌────┴────┐
    │         │
  active   not active
    │         │
    ▼         │
暂停空闲移动   │
    │         │
    ▼         │
执行 API 操作  │
    │         │
    ▼         │
恢复空闲移动   │
    │         │
    └────┬────┘
         │
         ▼
    返回响应
```

---

## 四、新增 HTTP API

### 4.1 空闲移动 API

#### POST /api/mouse/idle/start

启动空闲移动

**请求**:
```json
{
    "window": {
        "title": "微信",
        "className": "mmui::MainWindow"
    },
    "xpath": "//Pane[@ClassName='ChatView']",
    "speed": "normal",
    "moveInterval": 800,
    "idleTimeout": 60000,
    "humanIntervention": {
        "enabled": true,
        "pauseOnMouse": true,
        "pauseOnKeyboard": true,
        "resumeDelay": 3000
    }
}
```

**响应**:
```json
{
    "success": true,
    "error": null
}
```

#### POST /api/mouse/idle/stop

停止空闲移动

**响应**:
```json
{
    "success": true,
    "durationMs": 45000,
    "error": null
}
```

#### GET /api/mouse/idle/status

获取空闲移动状态

**响应**:
```json
{
    "active": true,
    "paused": false,
    "pauseReason": null,
    "currentRect": {
        "x": 100,
        "y": 200,
        "width": 500,
        "height": 400
    },
    "runningDurationMs": 45000,
    "lastActivityMs": 2000
}
```

### 4.2 键盘 API

#### POST /api/keyboard/type

拟人化打字

**请求**:
```json
{
    "text": "Hello World!",
    "charDelay": {
        "min": 50,
        "max": 150
    }
}
```

**响应**:
```json
{
    "success": true,
    "charsTyped": 12,
    "durationMs": 850,
    "error": null
}
```

---

## 五、服务端实现设计

### 5.1 全局状态管理

```rust
// src/api/idle_motion.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Instant;
use once_cell::sync::Lazy;

/// 空闲移动状态
pub struct IdleMotionState {
    // 基础状态
    pub active: bool,
    pub paused: bool,
    pub params: Option<IdleMotionParams>,
    pub cancel_token: Option<CancellationToken>,
    pub current_rect: Option<Rect>,
    
    // 服务端控制标志
    pub server_moving_mouse: bool,  // 标记服务端正在移动鼠标
    
    // 时间戳记录
    pub started_at: Option<Instant>,
    pub last_api_call: Option<Instant>,
    pub last_human_activity: Option<Instant>,
    
    // 暂停原因
    pub pause_reason: Option<PauseReason>,
}

/// 全局状态
pub static IDLE_STATE: Lazy<Arc<RwLock<IdleMotionState>>> = Lazy::new(|| {
    Arc::new(RwLock::new(IdleMotionState::default()))
});
```

### 5.2 自动暂停/恢复包装函数

```rust
/// 执行操作时自动暂停空闲移动，完成后自动恢复
pub async fn with_auto_pause<F, T>(f: F) -> T
where
    F: Future<Output = T>,
{
    // 1. 暂停空闲移动
    pause_idle_motion_internal(PauseReason::ApiCall).await;
    
    // 2. 记录 API 调用时间
    update_last_api_call().await;
    
    // 3. 执行实际操作
    let result = f.await;
    
    // 4. 恢复空闲移动
    resume_idle_motion_internal().await;
    
    result
}
```

### 5.3 人工干预检测（后台任务）

```rust
/// 检测人工鼠标移动的后台任务
async fn human_mouse_monitor(state: Arc<RwLock<IdleMotionState>>) {
    let mut last_mouse_pos = get_cursor_position();
    
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let state_guard = state.read().await;
        
        // 检查是否启用人工干预检测
        if !state_guard.active || 
           !state_guard.params.as_ref()
               .map(|p| p.human_intervention.enabled)
               .unwrap_or(false) {
            continue;
        }
        
        // 如果服务端正在移动鼠标，跳过检测
        if state_guard.server_moving_mouse {
            continue;
        }
        
        drop(state_guard);
        
        // 检测鼠标位置变化
        let current_pos = get_cursor_position();
        if current_pos != last_mouse_pos {
            // 检测到人工鼠标移动
            pause_idle_motion_internal(PauseReason::HumanMouseMove).await;
            update_last_human_activity().await;
        }
        
        last_mouse_pos = current_pos;
    }
}

/// 检测是否应该恢复空闲移动
async fn auto_resume_monitor(state: Arc<RwLock<IdleMotionState>>) {
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        let state_guard = state.read().await;
        
        if !state_guard.active || !state_guard.paused {
            continue;
        }
        
        // 只处理人工干预导致的暂停
        if !matches!(state_guard.pause_reason, 
            Some(PauseReason::HumanMouseMove) | 
            Some(PauseReason::HumanKeyboard)) {
            continue;
        }
        
        let resume_delay = state_guard.params.as_ref()
            .map(|p| p.human_intervention.resume_delay)
            .unwrap_or(3000);
        
        let last_human = state_guard.last_human_activity;
        
        if let Some(last) = last_human {
            // 用户静止超过 resume_delay，自动恢复
            if last.elapsed() > Duration::from_millis(resume_delay) {
                drop(state_guard);
                resume_idle_motion_internal().await;
                log::info!("Idle motion auto-resumed after user inactivity");
            }
        }
    }
}

/// 检测空闲超时
async fn idle_timeout_monitor(state: Arc<RwLock<IdleMotionState>>) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        let state_guard = state.read().await;
        
        if !state_guard.active {
            continue;
        }
        
        let idle_timeout = state_guard.params.as_ref()
            .map(|p| p.idle_timeout)
            .unwrap_or(60000);
        
        // 0 表示不超时
        if idle_timeout == 0 {
            continue;
        }
        
        let last_api = state_guard.last_api_call
            .or(state_guard.started_at)
            .unwrap_or(Instant::now());
        
        // 无操作超时，自动停止
        if last_api.elapsed() > Duration::from_millis(idle_timeout) {
            drop(state_guard);
            stop_idle_motion_internal().await;
            log::info!("Idle motion stopped due to inactivity timeout");
        }
    }
}
```

### 5.4 现有 API 改造

```rust
// src/api/mouse.rs

/// POST /api/mouse/click - 改造后
pub async fn click_mouse(body: web::Json<MouseClickRequest>) -> impl Responder {
    with_auto_pause(async move {
        // 原有的点击逻辑...
        let request = body.into_inner();
        // ... 实现点击
    }).await
}

/// POST /api/mouse/move - 改造后
pub async fn move_mouse(body: web::Json<MouseMoveRequest>) -> impl Responder {
    with_auto_pause(async move {
        // 原有的移动逻辑...
    }).await
}
```

---

## 六、Node.js SDK 设计

### 6.1 项目结构

```
sdk/nodejs/
├── src/
│   ├── index.ts              # 入口，导出主类和类型
│   ├── client.ts             # HTTP 客户端封装
│   ├── humanize-context.ts   # 拟人上下文
│   ├── types.ts              # TypeScript 类型定义
│   └── utils.ts              # 工具函数
├── package.json
├── tsconfig.json
└── DESIGN.md                 # 本设计文档
```

### 6.2 主类 API

```typescript
// ═══════════════════════════════════════════════════════════
// ElementSelectorSDK 主类
// ═══════════════════════════════════════════════════════════

export class ElementSelectorSDK {
    constructor(config: SDKConfig);
    
    // ─── 基础 API ───
    health(): Promise<HealthStatus>;
    listWindows(): Promise<WindowInfo[]>;
    getElement(params: ElementQueryParams): Promise<ElementInfo>;
    moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult>;
    click(params: ClickParams): Promise<ClickResult>;
    type(text: string, options?: TypeOptions): Promise<TypeResult>;
    
    // ─── 拟人上下文 ───
    humanize<T>(callback: (ctx: HumanizeContext) => Promise<T>): Promise<T>;
    
    // ─── 空闲移动 ───
    startIdleMotion(params: IdleMotionParams): Promise<void>;
    stopIdleMotion(): Promise<StopResult>;
    getIdleMotionStatus(): Promise<IdleMotionStatus>;
}
```

### 6.3 拟人上下文

```typescript
// ═══════════════════════════════════════════════════════════
// HumanizeContext - 在 humanize() 回调中使用
// ═══════════════════════════════════════════════════════════

export class HumanizeContext {
    constructor(private sdk: ElementSelectorSDK);
    
    // 所有方法都会自动应用默认拟人化参数
    click(params: ClickParams): Promise<ClickResult>;
    move(params: MoveParams): Promise<MoveResult>;
    type(text: string, options?: TypeOptions): Promise<TypeResult>;
    getElement(params: ElementQueryParams): Promise<ElementInfo>;
    
    // 链式调用支持
    chain(): ActionChain;
}

// ═══════════════════════════════════════════════════════════
// ActionChain - 链式调用
// ═══════════════════════════════════════════════════════════

export class ActionChain {
    click(params: ClickParams): this;
    type(text: string, options?: TypeOptions): this;
    move(target: Point): this;
    wait(ms: number): this;
    execute(): Promise<void>;
}
```

### 6.4 类型定义

```typescript
// ═══════════════════════════════════════════════════════════
// 基础类型
// ═══════════════════════════════════════════════════════════

export interface Point {
    x: number;
    y: number;
}

export interface Rect {
    x: number;
    y: number;
    width: number;
    height: number;
}

export interface WindowSelector {
    title?: string;
    className?: string;
    processName?: string;
}

export interface WindowInfo {
    title: string;
    className: string;
    processId: number;
    processName: string;
}

// ═══════════════════════════════════════════════════════════
// 空闲移动参数
// ═══════════════════════════════════════════════════════════

export interface IdleMotionParams {
    window: WindowSelector;
    xpath: string;
    
    // 移动参数
    speed?: 'slow' | 'normal' | 'fast';
    moveInterval?: number;       // 移动间隔，默认 800ms
    
    // 超时参数
    idleTimeout?: number;        // 无操作超时停止，默认 60000ms，0 表示不超时
    
    // 人工干预检测
    humanIntervention?: {
        enabled: boolean;
        pauseOnMouse?: boolean;      // 检测到人移动鼠标时暂停，默认 true
        pauseOnKeyboard?: boolean;   // 检测到人按键盘时暂停，默认 true
        resumeDelay?: number;        // 用户静止后恢复延迟，默认 3000ms
    };
}

export interface IdleMotionStatus {
    active: boolean;
    paused: boolean;
    pauseReason: 'api_call' | 'human_mouse' | 'human_keyboard' | 'manual' | null;
    currentRect: Rect | null;
    runningDurationMs: number | null;
    lastActivityMs: number | null;
}

// ═══════════════════════════════════════════════════════════
// 操作参数
// ═══════════════════════════════════════════════════════════

export interface ClickParams {
    window: WindowSelector;
    xpath: string;
    options?: ClickOptions;
}

export interface ClickOptions {
    humanize?: boolean;         // 默认 true
    randomRange?: number;       // 随机坐标范围，默认 0.55
    pauseBefore?: number;       // 点击前停顿
    pauseAfter?: number;        // 点击后停顿
}

export interface TypeOptions {
    charDelay?: {
        min: number;
        max: number;
    };
}

export interface MoveOptions {
    humanize?: boolean;
    trajectory?: 'linear' | 'bezier';
    duration?: number;
}
```

---

## 七、使用示例

### 7.1 基本使用

```typescript
import { ElementSelectorSDK } from 'element-selector-sdk';

const sdk = new ElementSelectorSDK({
    baseUrl: 'http://127.0.0.1:8080',
    timeout: 30000
});

// 检查服务状态
const health = await sdk.health();
console.log('Server status:', health.status);

// 列出窗口
const windows = await sdk.listWindows();
console.log('Windows:', windows);

// 查找元素
const element = await sdk.getElement({
    windowSelector: "Window[@Name='微信']",
    xpath: '//Button[@AutomationId="btnSend"]'
});
console.log('Element:', element);
```

### 7.2 拟人化操作

```typescript
// 使用拟人上下文
await sdk.humanize(async (ctx) => {
    // 点击元素（自动应用拟人化参数）
    await ctx.click({
        window: { title: '微信' },
        xpath: '//Button[@Name="发送"]'
    });
    
    // 打字（每个字符随机延迟）
    await ctx.type('Hello World!', {
        charDelay: { min: 50, max: 150 }
    });
});

// 链式调用
await sdk.humanize(async (ctx) => {
    await ctx.chain()
        .click({ window: { title: '微信' }, xpath: '//Edit' })
        .type('消息内容')
        .wait(500)
        .click({ window: { title: '微信' }, xpath: '//Button[@Name="发送"]' })
        .execute();
});
```

### 7.3 空闲移动完整流程

```typescript
// 启动智能空闲移动
await sdk.startIdleMotion({
    window: { title: '微信' },
    xpath: '//Pane[@ClassName="ChatView"]',
    
    speed: 'normal',
    moveInterval: 800,
    
    // 60秒无操作自动停止
    idleTimeout: 60000,
    
    // 人工干预检测
    humanIntervention: {
        enabled: true,
        pauseOnMouse: true,
        pauseOnKeyboard: true,
        resumeDelay: 3000
    }
});

// 所有操作自动暂停→执行→恢复
await sdk.click({ window: { title: '微信' }, xpath: '//Edit' });
await sdk.type('自动回复消息');
await sdk.click({ window: { title: '微信' }, xpath: '//Button[@Name="发送"]' });

// 如果用户中途移动鼠标，空闲移动会自动暂停
// 用户静止 3 秒后，空闲移动自动恢复

// 查询状态
const status = await sdk.getIdleMotionStatus();
console.log('Idle motion status:', status);

// 停止空闲移动
await sdk.stopIdleMotion();
```

---

## 八、实现计划

### 8.1 Phase 1: 服务端基础扩展

1. 新增 `src/api/idle_motion.rs` 模块
2. 实现全局状态管理 `IdleMotionState`
3. 实现 `with_auto_pause` 包装函数
4. 新增空闲移动 API 接口
5. 改造现有 mouse/click API

### 8.2 Phase 2: 后台监控任务

1. 实现人工鼠标移动检测
2. 实现自动恢复检测
3. 实现空闲超时检测
4. 启动后台任务管理

### 8.3 Phase 3: 键盘 API

1. 新增 `src/api/keyboard.rs` 模块
2. 实现拟人化打字 API
3. 集成自动暂停/恢复机制

### 8.4 Phase 4: Node.js SDK

1. 创建项目结构
2. 实现 HTTP 客户端
3. 实现拟人上下文
4. 实现类型定义
5. 编写使用文档和示例

---

## 九、配置默认值

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `speed` | `normal` | 移动速度 |
| `moveInterval` | `800ms` | 移动间隔 |
| `idleTimeout` | `60000ms` | 无操作超时 |
| `humanIntervention.enabled` | `true` | 启用人工干预检测 |
| `humanIntervention.pauseOnMouse` | `true` | 检测鼠标暂停 |
| `humanIntervention.pauseOnKeyboard` | `true` | 检测键盘暂停 |
| `humanIntervention.resumeDelay` | `3000ms` | 自动恢复延迟 |
| `clickOptions.randomRange` | `0.55` | 随机点击范围 |
| `clickOptions.pauseBefore` | `0ms` | 点击前停顿 |
| `clickOptions.pauseAfter` | `0ms` | 点击后停顿 |
| `typeOptions.charDelay.min` | `50ms` | 打字最小延迟 |
| `typeOptions.charDelay.max` | `150ms` | 打字最大延迟 |