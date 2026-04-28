import { SDKConfig, HealthStatus, WindowInfo, ElementQueryParams, ElementResponse, ElementInfo, MoveResult, ClickParams, ClickResult, IdleMotionParams, IdleMotionStatus, StopResult, Point, MoveOptions, TypeOptions, TypeResult } from './types';
export declare class HttpClient {
    private client;
    constructor(config: SDKConfig);
    health(): Promise<HealthStatus>;
    listWindows(): Promise<WindowInfo[]>;
    getElement(params: ElementQueryParams): Promise<ElementResponse>;
    moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult>;
    clickMouse(params: ClickParams): Promise<ClickResult>;
    startIdleMotion(params: IdleMotionParams): Promise<void>;
    stopIdleMotion(): Promise<StopResult>;
    getIdleMotionStatus(): Promise<IdleMotionStatus>;
    typeText(text: string, options?: TypeOptions): Promise<TypeResult>;
    /**
     * 激活指定窗口（使其成为前台窗口）
     * @param windowSelector 窗口选择器 XPath
     * @returns 激活结果
     */
    activateWindow(windowSelector: string): Promise<{
        success: boolean;
        error?: string;
    }>;
    /**
     * 激活窗口并使指定元素获得焦点
     * @param windowSelector 窗口选择器 XPath
     * @param xpath 元素 XPath
     * @returns 操作结果
     */
    focusElement(windowSelector: string, xpath: string): Promise<{
        success: boolean;
        error?: string;
    }>;
    /**
     * 获取所有匹配元素
     * @param params 查询参数
     * @returns 所有匹配的元素列表
     */
    getAllElements(params: ElementQueryParams): Promise<{
        found: boolean;
        elements: ElementInfo[];
        total: number;
        error?: string;
    }>;
    /**
     * 执行快捷键组合
     * @param keys 快捷键字符串，如 "Ctrl+C", "Alt+F4"
     */
    executeShortcut(keys: string): Promise<{
        success: boolean;
        error?: string;
    }>;
    /**
     * 执行单个按键
     * @param key 按键名称，如 "Enter", "Tab", "Escape"
     */
    executeKey(key: string): Promise<{
        success: boolean;
        error?: string;
    }>;
    handleError(error: unknown): Error;
}
//# sourceMappingURL=client.d.ts.map