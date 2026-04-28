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
    // 窗口激活 API
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 激活指定窗口（使其成为前台窗口）
     * 
     * **重要**: 在执行 click/type 等操作前，应先激活目标窗口以确保操作成功。
     * 
     * @param windowSelector 窗口选择器 XPath 或 WindowSelector 对象
     * @returns 激活结果
     * @example
     * // 使用 XPath 格式
     * await sdk.activateWindow("Window[@Name='微信' and @ClassName='mmui::MainWindow']");
     * 
     * // 使用 WindowSelector 对象
     * await sdk.activateWindow({ title: '微信', className: 'mmui::MainWindow' });
     */
    async activateWindow(windowSelector: string | WindowSelector): Promise<{ success: boolean; error?: string }> {
        const selector = typeof windowSelector === 'string' 
            ? windowSelector 
            : buildWindowSelector(windowSelector);
        return this.client.activateWindow(selector);
    }
    
    /**
     * 激活窗口并使指定元素获得焦点
     * 
     * 这是一站式方法，先激活窗口，然后聚焦目标元素。
     * 适用于需要在特定输入框中打字的场景。
     * 
     * @param windowSelector 窗口选择器
     * @param xpath 元素 XPath
     * @returns 操作结果
     * @example
     * await sdk.focusElement({ title: '微信' }, '//Edit[@Name="输入"]');
     * await sdk.type('消息内容');  // 现在焦点在输入框中
     */
    async focusElement(windowSelector: string | WindowSelector, xpath: string): Promise<{ success: boolean; error?: string }> {
        const selector = typeof windowSelector === 'string' 
            ? windowSelector 
            : buildWindowSelector(windowSelector);
        return this.client.focusElement(selector, xpath);
    }
    
    /**
     * 安全点击：先激活窗口，再点击元素
     * 
     * @param params 点击参数
     * @returns 点击结果
     */
    async safeClick(params: ClickParams): Promise<ClickResult> {
        // 构建窗口选择器
        const windowSelector = buildWindowSelector(params.window);
        
        // 先激活窗口
        await this.client.activateWindow(windowSelector);
        
        // 等待窗口激活
        await new Promise(r => setTimeout(r, 100));
        
        // 再点击
        return this.client.clickMouse(params);
    }
    
    /**
     * 安全打字：先激活窗口并聚焦元素，再打字
     * 
     * @param windowSelector 窗口选择器
     * @param xpath 目标输入元素 XPath
     * @param text 要打字的文本
     * @param options 打字选项
     * @returns 操作结果
     */
    async safeType(
        windowSelector: string | WindowSelector,
        xpath: string,
        text: string,
        options?: TypeOptions
    ): Promise<TypeResult> {
        const selector = typeof windowSelector === 'string' 
            ? windowSelector 
            : buildWindowSelector(windowSelector);
        
        // 先激活窗口并聚焦元素
        await this.client.focusElement(selector, xpath);
        
        // 等待焦点切换
        await new Promise(r => setTimeout(r, 100));
        
        // 再打字
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