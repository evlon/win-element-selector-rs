"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.randomFloat = exports.randomInt = exports.sleep = exports.buildWindowSelector = exports.HttpClient = exports.ActionChain = exports.HumanizeContext = exports.ElementSelectorSDK = void 0;
const client_1 = require("./client");
const humanize_context_1 = require("./humanize-context");
const types_1 = require("./types");
const utils_1 = require("./utils");
class ElementSelectorSDK {
    constructor(config) {
        this.client = new client_1.HttpClient({
            baseUrl: config?.baseUrl ?? types_1.DEFAULTS.baseUrl,
            timeout: config?.timeout ?? types_1.DEFAULTS.timeout,
        });
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 基础 API
    // ═══════════════════════════════════════════════════════════════════════════
    async health() {
        return this.client.health();
    }
    async listWindows() {
        return this.client.listWindows();
    }
    async getElement(params) {
        return this.client.getElement(params);
    }
    async moveMouse(target, options) {
        return this.client.moveMouse(target, options);
    }
    async click(params) {
        return this.client.clickMouse(params);
    }
    async type(text, options) {
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
    async activateWindow(windowSelector) {
        const selector = typeof windowSelector === 'string'
            ? windowSelector
            : (0, utils_1.buildWindowSelector)(windowSelector);
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
    async focusElement(windowSelector, xpath) {
        const selector = typeof windowSelector === 'string'
            ? windowSelector
            : (0, utils_1.buildWindowSelector)(windowSelector);
        return this.client.focusElement(selector, xpath);
    }
    /**
     * 安全点击：先激活窗口，再点击元素
     *
     * @param params 点击参数
     * @returns 点击结果
     */
    async safeClick(params) {
        // 构建窗口选择器
        const windowSelector = (0, utils_1.buildWindowSelector)(params.window);
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
    async safeType(windowSelector, xpath, text, options) {
        const selector = typeof windowSelector === 'string'
            ? windowSelector
            : (0, utils_1.buildWindowSelector)(windowSelector);
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
    async humanize(callback) {
        const ctx = new humanize_context_1.HumanizeContext(this.client);
        return callback(ctx);
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 空闲移动
    // ═══════════════════════════════════════════════════════════════════════════
    async startIdleMotion(params) {
        return this.client.startIdleMotion(params);
    }
    async stopIdleMotion() {
        return this.client.stopIdleMotion();
    }
    async getIdleMotionStatus() {
        return this.client.getIdleMotionStatus();
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 便捷方法
    // ═══════════════════════════════════════════════════════════════════════════
    static buildWindowSelector(selector) {
        return (0, utils_1.buildWindowSelector)(selector);
    }
}
exports.ElementSelectorSDK = ElementSelectorSDK;
// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════
__exportStar(require("./types"), exports);
var humanize_context_2 = require("./humanize-context");
Object.defineProperty(exports, "HumanizeContext", { enumerable: true, get: function () { return humanize_context_2.HumanizeContext; } });
var action_chain_1 = require("./action-chain");
Object.defineProperty(exports, "ActionChain", { enumerable: true, get: function () { return action_chain_1.ActionChain; } });
var client_2 = require("./client");
Object.defineProperty(exports, "HttpClient", { enumerable: true, get: function () { return client_2.HttpClient; } });
var utils_2 = require("./utils");
Object.defineProperty(exports, "buildWindowSelector", { enumerable: true, get: function () { return utils_2.buildWindowSelector; } });
Object.defineProperty(exports, "sleep", { enumerable: true, get: function () { return utils_2.sleep; } });
Object.defineProperty(exports, "randomInt", { enumerable: true, get: function () { return utils_2.randomInt; } });
Object.defineProperty(exports, "randomFloat", { enumerable: true, get: function () { return utils_2.randomFloat; } });
exports.default = ElementSelectorSDK;
//# sourceMappingURL=index.js.map