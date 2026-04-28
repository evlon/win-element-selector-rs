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
export interface SDKConfig {
    baseUrl: string;
    timeout?: number;
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
export interface HealthStatus {
    status: string;
    version: string;
    service: string;
}
export declare const DEFAULTS: {
    baseUrl: string;
    timeout: number;
    move: {
        humanize: boolean;
        trajectory: "bezier";
        duration: number;
    };
    click: {
        humanize: boolean;
        randomRange: number;
        pauseBefore: number;
        pauseAfter: number;
    };
    idleMotion: {
        speed: "normal";
        moveInterval: number;
        idleTimeout: number;
        humanIntervention: {
            enabled: boolean;
            pauseOnMouse: boolean;
            pauseOnKeyboard: boolean;
            resumeDelay: number;
        };
    };
    type: {
        charDelay: {
            min: number;
            max: number;
        };
    };
};
//# sourceMappingURL=types.d.ts.map