import { HttpClient } from '../client';
import { WindowSelector, ElementInfo } from '../types';
export { ElementInfo } from '../types';
export declare class FluentChain {
    private client;
    private actions;
    private screenshotManager;
    private currentWindowSelector;
    private currentElement;
    private humanizeEnabled;
    private humanizeOptions;
    private debugMode;
    constructor(client: HttpClient);
    /**
     * 开启拟人化模式
     */
    humanize(options?: {
        speed?: 'slow' | 'normal' | 'fast';
    }): this;
    /**
     * 开启调试模式
     */
    debug(): this;
    /**
     * 激活窗口
     */
    window(selector: string | WindowSelector): this;
    /**
     * 查找元素 - 找不到自动截图退出
     */
    find(xpath: string): this;
    /**
     * 查找所有匹配元素（返回数组，不执行后续操作）
     */
    findAll(xpath: string): Promise<ElementInfo[]>;
    /**
     * 提取元素属性数组
     * @param attrs 要提取的属性列表 ['name', 'controlType', 'rect']
     */
    extract(xpath: string, attrs: string[]): Promise<Record<string, unknown>[]>;
    /**
     * 提取元素文本列表
     */
    extractList(xpath: string): Promise<string[]>;
    /**
     * 提取表格数据
     * TODO: 需要更复杂的逻辑处理表格结构
     */
    extractTable(xpath: string): Promise<string[][]>;
    /**
     * 点击当前元素
     */
    click(): this;
    /**
     * 双击
     */
    doubleClick(): this;
    /**
     * 右键点击
     */
    rightClick(): this;
    /**
     * 打字
     */
    type(text: string): this;
    /**
     * 等待指定时间
     */
    wait(ms: number, randomMax?: number): this;
    /**
     * 执行快捷键
     */
    shortcut(keys: string): this;
    /**
     * 执行单个按键
     */
    key(keyName: string): this;
    /**
     * 获取元素信息
     */
    inspect(): Promise<ElementInfo | null>;
    /**
     * 执行整条链
     */
    run(): Promise<void>;
    private executePrefixActions;
    private executeWindow;
    private executeFind;
    private executeAction;
    private executeClick;
    private executeType;
    private executeWait;
    private failWithScreenshot;
    private getHumanizedDuration;
    private getHumanizedCharDelay;
    private log;
}
//# sourceMappingURL=fluent-chain.d.ts.map