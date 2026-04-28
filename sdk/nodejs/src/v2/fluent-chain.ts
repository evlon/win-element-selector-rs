// sdk/nodejs/src/v2/fluent-chain.ts
// SDK V2 流式链式调用核心实现

import { HttpClient } from '../client';
import { WindowSelector, DEFAULTS, Point, Rect, ElementInfo } from '../types';
import { buildWindowSelector } from '../utils';
import { ScreenshotManager } from './screenshot';
import * as fs from 'fs';
import * as path from 'path';

// 使用 types.ts 中定义的 ElementInfo
export { ElementInfo } from '../types';

// ═══════════════════════════════════════════════════════════════════════════════
// 链式操作动作定义
// ═══════════════════════════════════════════════════════════════════════════════

interface ChainAction {
    type: 'window' | 'find' | 'click' | 'doubleClick' | 'rightClick' | 'type' | 'wait' | 'humanize' | 'shortcut' | 'key';
    params?: unknown;
    xpath?: string;
    text?: string;
    duration?: number;
    options?: unknown;
}

// ═══════════════════════════════════════════════════════════════════════════════
// FluentChain 类 - 流式链式调用
// ═══════════════════════════════════════════════════════════════════════════════

export class FluentChain {
    private client: HttpClient;
    private actions: ChainAction[] = [];
    private screenshotManager: ScreenshotManager;
    
    // 当前状态
    private currentWindowSelector: string | null = null;
    private currentElement: ElementInfo | null = null;
    private humanizeEnabled: boolean = false;
    private humanizeOptions: { speed?: 'slow' | 'normal' | 'fast' } = {};
    private debugMode: boolean = false;
    
    constructor(client: HttpClient) {
        this.client = client;
        this.screenshotManager = new ScreenshotManager();
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 初始化方法
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 开启拟人化模式
     */
    humanize(options?: { speed?: 'slow' | 'normal' | 'fast' }): this {
        this.humanizeEnabled = true;
        this.humanizeOptions = options ?? {};
        this.actions.push({ type: 'humanize', options });
        return this;
    }
    
    /**
     * 开启调试模式
     */
    debug(): this {
        this.debugMode = true;
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 窗口操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 激活窗口
     */
    window(selector: string | WindowSelector): this {
        const windowSelector = typeof selector === 'string' 
            ? selector 
            : buildWindowSelector(selector);
        this.currentWindowSelector = windowSelector;
        this.actions.push({ type: 'window', params: windowSelector });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 元素查找
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 查找元素 - 找不到自动截图退出
     */
    find(xpath: string): this {
        this.actions.push({ type: 'find', xpath });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 多元素查找和提取
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 查找所有匹配元素（返回数组，不执行后续操作）
     */
    async findAll(xpath: string): Promise<ElementInfo[]> {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        
        this.log(`findAll("${xpath}")`);
        
        const result = await this.client.getAllElements({
            windowSelector: this.currentWindowSelector,
            xpath,
            randomRange: DEFAULTS.click.randomRange,
        });
        
        if (!result.found) {
            this.log(`  → not found`);
            return [];
        }
        
        this.log(`  → found ${result.total} elements`);
        return result.elements;
    }
    
    /**
     * 提取元素属性数组
     * @param attrs 要提取的属性列表 ['name', 'controlType', 'rect']
     */
    async extract(xpath: string, attrs: string[]): Promise<Record<string, unknown>[]> {
        const elements = await this.findAll(xpath);
        return elements.map(elem => {
            const result: Record<string, unknown> = {};
            for (const attr of attrs) {
                if (attr in elem) {
                    result[attr] = elem[attr as keyof ElementInfo];
                }
            }
            return result;
        });
    }
    
    /**
     * 提取元素文本列表
     */
    async extractList(xpath: string): Promise<string[]> {
        const elements = await this.findAll(xpath);
        return elements.map(elem => elem.name);
    }
    
    /**
     * 提取表格数据
     * TODO: 需要更复杂的逻辑处理表格结构
     */
    async extractTable(xpath: string): Promise<string[][]> {
        // 简化实现：假设每行是一个元素
        const elements = await this.findAll(xpath);
        return elements.map(elem => [elem.name]);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 点击操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 点击当前元素
     */
    click(): this {
        this.actions.push({ type: 'click' });
        return this;
    }
    
    /**
     * 双击
     */
    doubleClick(): this {
        this.actions.push({ type: 'doubleClick' });
        return this;
    }
    
    /**
     * 右键点击
     */
    rightClick(): this {
        this.actions.push({ type: 'rightClick' });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 打字操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 打字
     */
    type(text: string): this {
        this.actions.push({ type: 'type', text });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 等待操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 等待指定时间
     */
    wait(ms: number, randomMax?: number): this {
        const duration = randomMax 
            ? ms + Math.random() * (randomMax - ms)
            : ms;
        this.actions.push({ type: 'wait', duration });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 快捷键操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 执行快捷键
     */
    shortcut(keys: string): this {
        this.actions.push({ type: 'shortcut', text: keys });
        return this;
    }
    
    /**
     * 执行单个按键
     */
    key(keyName: string): this {
        this.actions.push({ type: 'key', text: keyName });
        return this;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 查询操作
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 获取元素信息
     */
    async inspect(): Promise<ElementInfo | null> {
        await this.executePrefixActions();
        return this.currentElement;
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 执行
    // ═══════════════════════════════════════════════════════════════════════════
    
    /**
     * 执行整条链
     */
    async run(): Promise<void> {
        await this.executePrefixActions();
        
        for (const action of this.actions) {
            if (action.type === 'window' || action.type === 'find' || action.type === 'humanize') {
                continue;
            }
            await this.executeAction(action);
        }
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 内部实现
    // ═══════════════════════════════════════════════════════════════════════════
    
    private async executePrefixActions(): Promise<void> {
        for (const action of this.actions) {
            if (action.type === 'humanize') continue;
            if (action.type === 'window') await this.executeWindow(action.params as string);
            if (action.type === 'find') await this.executeFind(action.xpath!);
        }
    }
    
    private async executeWindow(windowSelector: string): Promise<void> {
        this.log(`window("${windowSelector}")`);
        const result = await this.client.activateWindow(windowSelector);
        if (!result.success) {
            await this.failWithScreenshot(`Window not found: ${windowSelector}`, 'window');
        }
        this.log(`  → window activated`);
    }
    
    private async executeFind(xpath: string): Promise<void> {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        this.log(`find("${xpath}")`);
        
        const result = await this.client.getElement({
            windowSelector: this.currentWindowSelector,
            xpath,
            randomRange: DEFAULTS.click.randomRange,
        });
        
        if (!result.found || !result.element) {
            await this.failWithScreenshot(
                `Element not found: ${xpath}`,
                'find',
                { windowSelector: this.currentWindowSelector, xpath }
            );
            return; // process.exit 已调用，但 TypeScript 需要明确的 return
        }
        
        // result.element 已检查不为 null
        const element = result.element!;
        this.currentElement = element;
        this.log(`  → found: rect(${element.rect.x}, ${element.rect.y}, ${element.rect.width}x${element.rect.height})`);
    }
    
    private async executeAction(action: ChainAction): Promise<void> {
        switch (action.type) {
            case 'click': await this.executeClick(); break;
            case 'doubleClick': await this.executeClick('double'); break;
            case 'rightClick': await this.executeClick('right'); break;
            case 'type': await this.executeType(action.text!); break;
            case 'wait': await this.executeWait(action.duration!); break;
            case 'shortcut': this.log(`shortcut("${action.text}") - TODO`); break;
            case 'key': this.log(`key("${action.text}") - TODO`); break;
        }
    }
    
    private async executeClick(mode: 'single' | 'double' | 'right' = 'single'): Promise<void> {
        if (!this.currentElement) throw new Error('必须先调用 find() 找到元素');
        const modeText = mode === 'single' ? 'click' : mode === 'double' ? 'doubleClick' : 'rightClick';
        this.log(`${modeText}()`);
        
        const target = this.currentElement.centerRandom;
        const duration = this.getHumanizedDuration();
        
        const result = await this.client.moveMouse(target, {
            humanize: this.humanizeEnabled,
            trajectory: DEFAULTS.move.trajectory,
            duration,
        });
        
        if (!result.success) {
            await this.failWithScreenshot(`${modeText} failed`, modeText);
            return;
        }
        this.log(`  → clicked at (${target.x}, ${target.y}), ${result.durationMs}ms`);
    }
    
    private async executeType(text: string): Promise<void> {
        this.log(`type("${text}")`);
        const charDelay = this.getHumanizedCharDelay();
        const result = await this.client.typeText(text, { charDelay });
        if (!result.success) await this.failWithScreenshot(`Type failed`, 'type');
        this.log(`  → typed ${result.charsTyped} chars, ${result.durationMs}ms`);
    }
    
    private async executeWait(duration: number): Promise<void> {
        this.log(`wait(${Math.round(duration)}ms)`);
        await new Promise(r => setTimeout(r, duration));
        this.log(`  → waited`);
    }
    
    private async failWithScreenshot(message: string, step: string, context?: { windowSelector?: string; xpath?: string }): Promise<void> {
        const screenshotPath = await this.screenshotManager.captureFailure(step);
        
        console.log('\n' + '='.repeat(60));
        console.log(`[FAILED] ${message}`);
        console.log('='.repeat(60));
        
        if (context?.windowSelector) console.log(`Window selector: ${context.windowSelector}`);
        if (context?.xpath) console.log(`XPath: ${context.xpath}`);
        
        console.log('\nAvailable windows:');
        const windows = await this.client.listWindows();
        windows.slice(0, 5).forEach(w => console.log(`  - ${w.title} (${w.className}, ${w.processName})`));
        
        console.log(`\nScreenshot saved: ${screenshotPath}`);
        console.log('Process exiting for manual intervention...\n');
        process.exit(1);
    }
    
    private getHumanizedDuration(): number {
        const base = DEFAULTS.move.duration;
        if (!this.humanizeEnabled) return base;
        const speedFactor = this.humanizeOptions.speed === 'slow' ? 1.5 : this.humanizeOptions.speed === 'fast' ? 0.5 : 1.0;
        return Math.round(base * speedFactor * (0.8 + Math.random() * 0.4));
    }
    
    private getHumanizedCharDelay(): { min: number; max: number } {
        if (!this.humanizeEnabled) return DEFAULTS.type.charDelay;
        const base = DEFAULTS.type.charDelay;
        const speedFactor = this.humanizeOptions.speed === 'slow' ? 2 : this.humanizeOptions.speed === 'fast' ? 0.5 : 1.0;
        return { min: Math.round(base.min * speedFactor), max: Math.round(base.max * speedFactor) };}
    
    private log(message: string): void {
        if (this.debugMode) console.log(`[DEBUG] ${message}`);
    }
}