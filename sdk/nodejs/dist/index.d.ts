import { HumanizeContext } from './humanize-context';
import { SDKConfig, HealthStatus, WindowInfo, WindowSelector, ElementQueryParams, ElementResponse, Point, MoveOptions, MoveResult, ClickParams, ClickResult, TypeOptions, TypeResult, IdleMotionParams, IdleMotionStatus, StopResult } from './types';
export declare class ElementSelectorSDK {
    private client;
    constructor(config?: Partial<SDKConfig>);
    health(): Promise<HealthStatus>;
    listWindows(): Promise<WindowInfo[]>;
    getElement(params: ElementQueryParams): Promise<ElementResponse>;
    moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult>;
    click(params: ClickParams): Promise<ClickResult>;
    type(text: string, options?: TypeOptions): Promise<TypeResult>;
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
    activateWindow(windowSelector: string | WindowSelector): Promise<{
        success: boolean;
        error?: string;
    }>;
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
    focusElement(windowSelector: string | WindowSelector, xpath: string): Promise<{
        success: boolean;
        error?: string;
    }>;
    /**
     * 安全点击：先激活窗口，再点击元素
     *
     * @param params 点击参数
     * @returns 点击结果
     */
    safeClick(params: ClickParams): Promise<ClickResult>;
    /**
     * 安全打字：先激活窗口并聚焦元素，再打字
     *
     * @param windowSelector 窗口选择器
     * @param xpath 目标输入元素 XPath
     * @param text 要打字的文本
     * @param options 打字选项
     * @returns 操作结果
     */
    safeType(windowSelector: string | WindowSelector, xpath: string, text: string, options?: TypeOptions): Promise<TypeResult>;
    humanize<T>(callback: (ctx: HumanizeContext) => Promise<T>): Promise<T>;
    startIdleMotion(params: IdleMotionParams): Promise<void>;
    stopIdleMotion(): Promise<StopResult>;
    getIdleMotionStatus(): Promise<IdleMotionStatus>;
    static buildWindowSelector(selector: WindowSelector): string;
}
export * from './types';
export { HumanizeContext } from './humanize-context';
export { ActionChain } from './action-chain';
export { HttpClient } from './client';
export { buildWindowSelector, sleep, randomInt, randomFloat } from './utils';
export default ElementSelectorSDK;
//# sourceMappingURL=index.d.ts.map