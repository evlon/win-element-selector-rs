import { FluentChain } from './fluent-chain';
import { SDKConfig, WindowSelector } from '../types';
export declare class ElementSelectorSDKv2 {
    private client;
    private screenshotManager;
    constructor(config?: Partial<SDKConfig>);
    /**
     * 创建流式链式调用
     *
     * @example
     * await sdk.chain()
     *     .humanize()
     *     .window("微信")
     *     .find("//Edit[@Name='输入']")
     *     .click()
     *     .type("你好")
     *     .run();
     */
    chain(): FluentChain;
    /**
     * 快捷方式：直接开始拟人化链式调用
     *
     * @example
     * await sdk
     *     .humanize()
     *     .window("微信")
     *     .find("//Edit")
     *     .click()
     *     .run();
     */
    humanize(options?: {
        speed?: 'slow' | 'normal' | 'fast';
    }): FluentChain;
    /**
     * 快捷方式：直接指定窗口开始链式调用
     */
    window(selector: string | WindowSelector): FluentChain;
    /**
     * 健康检查
     */
    health(): Promise<import("../types").HealthStatus>;
    /**
     * 获取窗口列表
     */
    listWindows(): Promise<import("../types").WindowInfo[]>;
    /**
     * 截图
     */
    screenshot(name?: string): Promise<string>;
}
export { FluentChain, ElementInfo } from './fluent-chain';
export { ScreenshotManager } from './screenshot';
export default ElementSelectorSDKv2;
//# sourceMappingURL=index.d.ts.map