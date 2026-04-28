import { HttpClient } from './client';
import { HumanizeContext } from './humanize-context';
import {
    SDKConfig,
    DEFAULTS,
    HealthStatus,
    WindowInfo,
    WindowSelector,
    ElementQueryParams,
    ElementResponse,
    Point,
    MoveOptions,
    MoveResult,
    ClickParams,
    ClickResult,
    TypeOptions,
    TypeResult,
    IdleMotionParams,
    IdleMotionStatus,
    StopResult,
} from './types';
import { buildWindowSelector } from './utils';

export class ElementSelectorSDK {
    private client: HttpClient;
    
    constructor(config?: Partial<SDKConfig>) {
        this.client = new HttpClient({
            baseUrl: config?.baseUrl ?? DEFAULTS.baseUrl,
            timeout: config?.timeout ?? DEFAULTS.timeout,
        });
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 基础 API
    // ═══════════════════════════════════════════════════════════════════════════
    
    async health(): Promise<HealthStatus> {
        return this.client.health();
    }
    
    async listWindows(): Promise<WindowInfo[]> {
        return this.client.listWindows();
    }
    
    async getElement(params: ElementQueryParams): Promise<ElementResponse> {
        return this.client.getElement(params);
    }
    
    async moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult> {
        return this.client.moveMouse(target, options);
    }
    
    async click(params: ClickParams): Promise<ClickResult> {
        return this.client.clickMouse(params);
    }
    
    async type(text: string, options?: TypeOptions): Promise<TypeResult> {
        return this.client.typeText(text, options);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 拟人上下文
    // ═══════════════════════════════════════════════════════════════════════════
    
    async humanize<T>(callback: (ctx: HumanizeContext) => Promise<T>): Promise<T> {
        const ctx = new HumanizeContext(this.client);
        return callback(ctx);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 空闲移动
    // ═══════════════════════════════════════════════════════════════════════════
    
    async startIdleMotion(params: IdleMotionParams): Promise<void> {
        return this.client.startIdleMotion(params);
    }
    
    async stopIdleMotion(): Promise<StopResult> {
        return this.client.stopIdleMotion();
    }
    
    async getIdleMotionStatus(): Promise<IdleMotionStatus> {
        return this.client.getIdleMotionStatus();
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 便捷方法
    // ═══════════════════════════════════════════════════════════════════════════
    
    static buildWindowSelector(selector: WindowSelector): string {
        return buildWindowSelector(selector);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════

export * from './types';
export { HumanizeContext } from './humanize-context';
export { ActionChain } from './action-chain';
export { HttpClient } from './client';
export { buildWindowSelector, sleep, randomInt, randomFloat } from './utils';

export default ElementSelectorSDK;