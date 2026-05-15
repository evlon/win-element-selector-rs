// sdk/nodejs/src/errors.ts
// 企业级异常定义

/**
 * SDK 基础异常类
 */
export class SDKError extends Error {
    public readonly code: string;
    public readonly context?: Record<string, any>;
    public readonly timestamp: number;

    constructor(
        message: string,
        code: string,
        context?: Record<string, any>
    ) {
        super(message);
        this.name = 'SDKError';
        this.code = code;
        this.context = context;
        this.timestamp = Date.now();
        
        // 保持正确的原型链
        Object.setPrototypeOf(this, SDKError.prototype);
    }

    /**
     * 转换为 JSON（用于日志记录）
     */
    toJSON(): Record<string, any> {
        return {
            name: this.name,
            message: this.message,
            code: this.code,
            context: this.context,
            timestamp: new Date(this.timestamp).toISOString(),
            stack: this.stack,
        };
    }
}

/**
 * 元素未找到异常
 */
export class ElementNotFoundError extends SDKError {
    constructor(
        xpath: string,
        windowSelector: string,
        screenshotPath?: string
    ) {
        super(
            `Element not found: ${xpath}`,
            'ELEMENT_NOT_FOUND',
            { 
                xpath, 
                windowSelector, 
                screenshotPath,
                hint: 'Check if the element exists and the XPath is correct'
            }
        );
        this.name = 'ElementNotFoundError';
        Object.setPrototypeOf(this, ElementNotFoundError.prototype);
    }
}

/**
 * 窗口未找到异常
 */
export class WindowNotFoundError extends SDKError {
    constructor(windowSelector: string) {
        super(
            `Window not found: ${windowSelector}`,
            'WINDOW_NOT_FOUND',
            { 
                windowSelector,
                hint: 'Check if the window is open and the selector is correct'
            }
        );
        this.name = 'WindowNotFoundError';
        Object.setPrototypeOf(this, WindowNotFoundError.prototype);
    }
}

/**
 * 网络错误异常
 */
export class NetworkError extends SDKError {
    constructor(
        originalError: Error,
        endpoint: string,
        statusCode?: number
    ) {
        super(
            `Network error: ${originalError.message}`,
            'NETWORK_ERROR',
            { 
                endpoint, 
                statusCode,
                originalMessage: originalError.message,
                originalStack: originalError.stack,
                hint: 'Check if the server is running and network connection is stable'
            }
        );
        this.name = 'NetworkError';
        Object.setPrototypeOf(this, NetworkError.prototype);
    }
}

/**
 * 超时异常
 */
export class TimeoutError extends SDKError {
    constructor(
        operation: string,
        timeout: number
    ) {
        super(
            `Operation timed out: ${operation} (${timeout}ms)`,
            'TIMEOUT_ERROR',
            { 
                operation, 
                timeout,
                hint: 'Consider increasing the timeout or optimizing the operation'
            }
        );
        this.name = 'TimeoutError';
        Object.setPrototypeOf(this, TimeoutError.prototype);
    }
}

/**
 * 动作执行失败异常
 */
export class ActionFailedError extends SDKError {
    constructor(
        action: string,
        reason: string,
        screenshotPath?: string
    ) {
        super(
            `Action failed: ${action} - ${reason}`,
            `ACTION_FAILED_${action.toUpperCase()}`,
            { 
                action, 
                reason,
                screenshotPath,
                hint: 'Check the screenshot for details'
            }
        );
        this.name = 'ActionFailedError';
        Object.setPrototypeOf(this, ActionFailedError.prototype);
    }
}

/**
 * 无效参数异常
 */
export class InvalidArgumentError extends SDKError {
    constructor(
        parameter: string,
        reason: string
    ) {
        super(
            `Invalid argument: ${parameter} - ${reason}`,
            'INVALID_ARGUMENT',
            { 
                parameter, 
                reason,
                hint: 'Check the parameter value and type'
            }
        );
        this.name = 'InvalidArgumentError';
        Object.setPrototypeOf(this, InvalidArgumentError.prototype);
    }
}

/**
 * 状态错误异常（操作顺序错误等）
 */
export class StateError extends SDKError {
    constructor(
        message: string,
        currentState?: string
    ) {
        super(
            message,
            'STATE_ERROR',
            { 
                currentState,
                hint: 'Check if prerequisites are met before this operation'
            }
        );
        this.name = 'StateError';
        Object.setPrototypeOf(this, StateError.prototype);
    }
}

/**
 * 类型守卫：判断是否为 SDKError
 */
export function isSDKError(error: unknown): error is SDKError {
    return error instanceof SDKError;
}

/**
 * 类型守卫：判断是否为 ElementNotFoundError
 */
export function isElementNotFoundError(error: unknown): error is ElementNotFoundError {
    return error instanceof ElementNotFoundError;
}

/**
 * 类型守卫：判断是否为 WindowNotFoundError
 */
export function isWindowNotFoundError(error: unknown): error is WindowNotFoundError {
    return error instanceof WindowNotFoundError;
}
