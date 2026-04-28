// ═══════════════════════════════════════════════════════════════════════════════
// 基础类型
// ═══════════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════════
// SDK 配置
// ═══════════════════════════════════════════════════════════════════════════════

export interface SDKConfig {
    baseUrl: string;
    timeout?: number;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 窗口相关
// ═══════════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════════
// 元素相关
// ═══════════════════════════════════════════════════════════════════════════════

export interface ElementQueryParams {
    windowSelector: string;
    xpath: string;
    randomRange?: number;
}

export interface ElementInfo {
    rect: Rect;
    center: Point;
    centerRandom: Point;
    controlType: string;
    name: string;
    isEnabled: boolean;
}

export interface ElementResponse {
    found: boolean;
    element: ElementInfo | null;
    error: string | null;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 鼠标操作相关
// ═══════════════════════════════════════════════════════════════════════════════

export interface MoveOptions {
    humanize?: boolean;
    trajectory?: 'linear' | 'bezier';
    duration?: number;
}

export interface MoveParams {
    target: Point;
    options?: MoveOptions;
}

export interface MoveResult {
    success: boolean;
    startPoint: Point;
    endPoint: Point;
    durationMs: number;
    error: string | null;
}

export interface ClickOptions {
    humanize?: boolean;
    randomRange?: number;
    pauseBefore?: number;
    pauseAfter?: number;
}

export interface ClickParams {
    window: WindowSelector;
    xpath: string;
    options?: ClickOptions;
}

export interface ClickedElement {
    controlType: string;
    name: string;
}

export interface ClickResult {
    success: boolean;
    clickPoint: Point;
    element: ClickedElement | null;
    error: string | null;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 键盘操作相关
// ═══════════════════════════════════════════════════════════════════════════════

export interface TypeOptions {
    charDelay?: {
        min: number;
        max: number;
    };
}

export interface TypeResult {
    success: boolean;
    charsTyped: number;
    durationMs: number;
    error: string | null;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 空闲移动相关
// ═══════════════════════════════════════════════════════════════════════════════

export interface HumanInterventionConfig {
    enabled: boolean;
    pauseOnMouse?: boolean;
    pauseOnKeyboard?: boolean;
    resumeDelay?: number;
}

export interface IdleMotionParams {
    window: WindowSelector;
    xpath: string;
    speed?: 'slow' | 'normal' | 'fast';
    moveInterval?: number;
    idleTimeout?: number;
    humanIntervention?: HumanInterventionConfig;
}

export type PauseReason = 'api_call' | 'human_mouse' | 'human_keyboard' | 'manual' | null;

export interface IdleMotionStatus {
    active: boolean;
    paused: boolean;
    pauseReason: PauseReason;
    currentRect: Rect | null;
    runningDurationMs: number | null;
    lastActivityMs: number | null;
}

export interface StopResult {
    success: boolean;
    durationMs: number;
    error: string | null;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 健康检查
// ═══════════════════════════════════════════════════════════════════════════════

export interface HealthStatus {
    status: string;
    version: string;
    service: string;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 默认值
// ═══════════════════════════════════════════════════════════════════════════════

export const DEFAULTS = {
    baseUrl: 'http://127.0.0.1:8080',
    timeout: 30000,
    
    move: {
        humanize: true,
        trajectory: 'bezier' as const,
        duration: 600,
    },
    
    click: {
        humanize: true,
        randomRange: 0.55,
        pauseBefore: 0,
        pauseAfter: 0,
    },
    
    idleMotion: {
        speed: 'normal' as const,
        moveInterval: 800,
        idleTimeout: 60000,
        humanIntervention: {
            enabled: true,
            pauseOnMouse: true,
            pauseOnKeyboard: true,
            resumeDelay: 3000,
        },
    },
    
    type: {
        charDelay: {
            min: 50,
            max: 150,
        },
    },
};