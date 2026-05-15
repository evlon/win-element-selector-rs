// sdk/nodejs/src/logger.ts
// 企业级日志模块 - 基于 pino

import pino from 'pino';

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'silent';

export interface LoggerOptions {
    level?: LogLevel;
    module?: string;
}

/**
 * 创建结构化日志记录器
 * 
 * @example
 * const logger = createLogger('HttpClient');
 * logger.info('Request completed', { duration: 100, status: 200 });
 * logger.error(error, 'Request failed', { url: '/api/element' });
 */
export class Logger {
    private logger: pino.Logger;
    private module: string;

    constructor(options: LoggerOptions = {}) {
        this.module = options.module || 'SDK';
        
        // 根据环境自动配置
        const isProduction = process.env.NODE_ENV === 'production';
        const defaultLevel: LogLevel = isProduction ? 'info' : 'debug';
        
        this.logger = pino({
            level: process.env.LOG_LEVEL || defaultLevel,
            transport: !isProduction ? {
                target: 'pino-pretty',
                options: {
                    colorize: true,
                    translateTime: 'SYS:standard',
                    ignore: 'pid,hostname',
                    messageFormat: '{module}: {msg}',
                    // Windows 终端兼容性：使用 UTF-8 编码
                    destination: 1, // stdout
                    mkdir: true,
                }
            } : undefined,
            base: undefined, // 不输出 pid, hostname 等基础信息
        });
        
        // 绑定模块名称到所有日志
        this.logger = this.logger.child({ module: this.module });
    }

    /**
     * TRACE 级别日志 - 最详细的调试信息
     */
    trace(msg: string, ...args: any[]): void {
        this.logger.trace(msg, ...args);
    }

    /**
     * DEBUG 级别日志 - 调试信息
     */
    debug(msg: string, ...args: any[]): void {
        this.logger.debug(msg, ...args);
    }

    /**
     * INFO 级别日志 - 关键操作信息
     */
    info(msg: string, ...args: any[]): void {
        this.logger.info(msg, ...args);
    }

    /**
     * WARN 级别日志 - 警告信息
     */
    warn(msg: string, ...args: any[]): void {
        this.logger.warn(msg, ...args);
    }

    /**
     * ERROR 级别日志 - 错误信息
     */
    error(msg: string, context?: Record<string, any>): void {
        this.logger.error(context || {}, msg);
    }

    /**
     * ERROR 级别日志 - 带 Error 对象
     */
    errorWithException(error: Error, msg?: string, context?: Record<string, any>): void {
        this.logger.error({ err: error, ...context }, msg || error.message);
    }

    /**
     * 动态调整日志级别
     */
    setLevel(level: LogLevel): void {
        this.logger.level = level;
    }

    /**
     * 获取当前日志级别
     */
    getLevel(): string {
        return this.logger.level;
    }
}

/**
 * 工厂函数：创建日志记录器
 */
export function createLogger(module: string, options?: LoggerOptions): Logger {
    return new Logger({ module, ...options });
}

/**
 * 全局日志配置
 */
export const LogConfig = {
    /**
     * 设置全局日志级别
     */
    setLevel(level: LogLevel): void {
        process.env.LOG_LEVEL = level;
    },

    /**
     * 获取当前全局日志级别
     */
    getLevel(): LogLevel {
        return (process.env.LOG_LEVEL as LogLevel) || 'debug';
    },

    /**
     * 启用生产模式（JSON 格式输出）
     */
    enableProduction(): void {
        process.env.NODE_ENV = 'production';
    },

    /**
     * 启用开发模式（美化输出）
     */
    enableDevelopment(): void {
        process.env.NODE_ENV = 'development';
    }
};
