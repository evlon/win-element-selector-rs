"use strict";
// sdk/nodejs/src/v2/index.ts
// SDK V2 - 流式 XPath 自动化 API
Object.defineProperty(exports, "__esModule", { value: true });
exports.ScreenshotManager = exports.FluentChain = exports.ElementSelectorSDKv2 = void 0;
const client_1 = require("../client");
const fluent_chain_1 = require("./fluent-chain");
const screenshot_1 = require("./screenshot");
const types_1 = require("../types");
// ═══════════════════════════════════════════════════════════════════════════════
// SDK V2 入口类
// ═══════════════════════════════════════════════════════════════════════════════
class ElementSelectorSDKv2 {
    constructor(config) {
        this.client = new client_1.HttpClient({
            baseUrl: config?.baseUrl ?? types_1.DEFAULTS.baseUrl,
            timeout: config?.timeout ?? types_1.DEFAULTS.timeout,
        });
        this.screenshotManager = screenshot_1.globalScreenshotManager;
    }
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
    chain() {
        return new fluent_chain_1.FluentChain(this.client);
    }
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
    humanize(options) {
        return this.chain().humanize(options);
    }
    /**
     * 快捷方式：直接指定窗口开始链式调用
     */
    window(selector) {
        return this.chain().window(selector);
    }
    // ═══════════════════════════════════════════════════════════════════════════════
    // 原有 API（兼容性保留）
    // ═══════════════════════════════════════════════════════════════════════════════
    /**
     * 健康检查
     */
    async health() {
        return this.client.health();
    }
    /**
     * 获取窗口列表
     */
    async listWindows() {
        return this.client.listWindows();
    }
    /**
     * 截图
     */
    async screenshot(name) {
        return this.screenshotManager.captureAuto();
    }
}
exports.ElementSelectorSDKv2 = ElementSelectorSDKv2;
// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════
var fluent_chain_2 = require("./fluent-chain");
Object.defineProperty(exports, "FluentChain", { enumerable: true, get: function () { return fluent_chain_2.FluentChain; } });
var screenshot_2 = require("./screenshot");
Object.defineProperty(exports, "ScreenshotManager", { enumerable: true, get: function () { return screenshot_2.ScreenshotManager; } });
// 默认导出
exports.default = ElementSelectorSDKv2;
//# sourceMappingURL=index.js.map