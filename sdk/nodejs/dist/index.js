"use strict";
// sdk/nodejs/src/index.ts
// Element Selector SDK - 流式 XPath 自动化
Object.defineProperty(exports, "__esModule", { value: true });
exports.buildWindowSelector = exports.DEFAULTS = exports.isWindowNotFoundError = exports.isElementNotFoundError = exports.isSDKError = exports.StateError = exports.InvalidArgumentError = exports.ActionFailedError = exports.TimeoutError = exports.NetworkError = exports.WindowNotFoundError = exports.ElementNotFoundError = exports.SDKError = exports.LogConfig = exports.Logger = exports.createLogger = exports.Chain = exports.SDK = void 0;
const client_1 = require("./client");
const chain_1 = require("./chain");
const types_1 = require("./types");
// ═══════════════════════════════════════════════════════════════════════════════
// SDK 入口
// ═══════════════════════════════════════════════════════════════════════════════
/**
 * Element Selector SDK
 *
 * 流式 XPath 自动化，简单直接，失败自动截图退出。
 *
 * @example
 * import { SDK } from 'element-selector-sdk';
 *
 * const sdk = new SDK();
 *
 * // 基础用法
 * await sdk.flow()
 *     .window("微信")
 *     .find("//Edit[@Name='输入']")
 *     .click()
 *     .type("你好")
 *     .run();
 *
 * // 拟人化
 * await sdk.flow()
 *     .humanize({ speed: 'slow' })
 *     .window("微信")
 *     .find("//Edit[@Name='输入']")
 *     .click()
 *     .type("你好")
 *     .run();
 *
 * // 等待元素
 * await sdk.flow()
 *     .window("Chrome")
 *     .waitFor("//Button[@Name='登录']", { timeout: 10000 })
 *     .click()
 *     .run();
 *
 * // 数据提取
 * const items = await sdk.flow().window("微信").findAll("//ListItem");
 * const texts = await sdk.flow().window("微信").extractList("//ListItem");
 */
class SDK {
    constructor(config) {
        this.client = new client_1.HttpClient({
            baseUrl: config?.baseUrl ?? types_1.DEFAULTS.baseUrl,
            timeout: config?.timeout ?? types_1.DEFAULTS.timeout,
        });
    }
    /**
     * 创建流式链式调用
     */
    flow() {
        return new chain_1.Chain(this.client);
    }
    /**
     * 快捷方式：开启拟人化
     */
    humanize(options) {
        return this.flow().humanize(options);
    }
    /**
     * 快捷方式：指定窗口
     */
    window(selector) {
        return this.flow().window(selector);
    }
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
}
exports.SDK = SDK;
// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════
// 类导出（值）
var chain_2 = require("./chain");
Object.defineProperty(exports, "Chain", { enumerable: true, get: function () { return chain_2.Chain; } });
// 日志相关导出
var logger_1 = require("./logger");
Object.defineProperty(exports, "createLogger", { enumerable: true, get: function () { return logger_1.createLogger; } });
Object.defineProperty(exports, "Logger", { enumerable: true, get: function () { return logger_1.Logger; } });
Object.defineProperty(exports, "LogConfig", { enumerable: true, get: function () { return logger_1.LogConfig; } });
// 异常相关导出
var errors_1 = require("./errors");
Object.defineProperty(exports, "SDKError", { enumerable: true, get: function () { return errors_1.SDKError; } });
Object.defineProperty(exports, "ElementNotFoundError", { enumerable: true, get: function () { return errors_1.ElementNotFoundError; } });
Object.defineProperty(exports, "WindowNotFoundError", { enumerable: true, get: function () { return errors_1.WindowNotFoundError; } });
Object.defineProperty(exports, "NetworkError", { enumerable: true, get: function () { return errors_1.NetworkError; } });
Object.defineProperty(exports, "TimeoutError", { enumerable: true, get: function () { return errors_1.TimeoutError; } });
Object.defineProperty(exports, "ActionFailedError", { enumerable: true, get: function () { return errors_1.ActionFailedError; } });
Object.defineProperty(exports, "InvalidArgumentError", { enumerable: true, get: function () { return errors_1.InvalidArgumentError; } });
Object.defineProperty(exports, "StateError", { enumerable: true, get: function () { return errors_1.StateError; } });
Object.defineProperty(exports, "isSDKError", { enumerable: true, get: function () { return errors_1.isSDKError; } });
Object.defineProperty(exports, "isElementNotFoundError", { enumerable: true, get: function () { return errors_1.isElementNotFoundError; } });
Object.defineProperty(exports, "isWindowNotFoundError", { enumerable: true, get: function () { return errors_1.isWindowNotFoundError; } });
var types_2 = require("./types");
Object.defineProperty(exports, "DEFAULTS", { enumerable: true, get: function () { return types_2.DEFAULTS; } });
// 工具导出
var utils_1 = require("./utils");
Object.defineProperty(exports, "buildWindowSelector", { enumerable: true, get: function () { return utils_1.buildWindowSelector; } });
// 默认导出
exports.default = SDK;
//# sourceMappingURL=index.js.map