import axios, { AxiosInstance, AxiosError } from 'axios';
import {
    SDKConfig,
    DEFAULTS,
    HealthStatus,
    WindowInfo,
    ElementQueryParams,
    ElementResponse,
    ElementInfo,
    MoveParams,
    MoveResult,
    ClickParams,
    ClickResult,
    IdleMotionParams,
    IdleMotionStatus,
    StopResult,
    Point,
    MoveOptions,
    TypeOptions,
    TypeResult,
} from './types';
import { createLogger, Logger } from './logger';
import { NetworkError, TimeoutError, SDKError } from './errors';

export class HttpClient {
    private client: AxiosInstance;
    private logger: Logger;
    
    constructor(config: SDKConfig) {
        this.client = axios.create({
            baseURL: config.baseUrl,
            timeout: config.timeout ?? DEFAULTS.timeout,
            headers: {
                'Content-Type': 'application/json',
            },
        });
        this.logger = createLogger('HttpClient');
    }
    
    async health(): Promise<HealthStatus> {
        const response = await this.client.get<HealthStatus>('/api/health');
        return response.data;
    }
    
    async listWindows(): Promise<WindowInfo[]> {
        const response = await this.client.post<{ windows: WindowInfo[] }>('/api/window/list');
        return response.data.windows;
    }
    
    async getElement(params: ElementQueryParams): Promise<ElementResponse> {
        const startTime = Date.now();
        this.logger.debug('GET /api/element', { 
            windowSelector: params.windowSelector.substring(0, 50) + '...',
            xpath: params.xpath.substring(0, 80) + '...' 
        });
        
        try {
            const response = await this.client.get<ElementResponse>('/api/element', {
                params: {
                    windowSelector: params.windowSelector,
                    xpath: params.xpath,
                    randomRange: params.randomRange ?? DEFAULTS.click.randomRange,
                },
            });
            
            const duration = Date.now() - startTime;
            this.logger.info('Element query completed', { 
                duration, 
                found: response.data.found 
            });
            
            return response.data;
        } catch (error) {
            this.logger.error('Element query failed', { params, error: (error as Error).message });
            throw this.handleError(error, '/api/element');
        }
    }
    
    async moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult> {
        const response = await this.client.post<MoveResult>('/api/mouse/move', {
            target,
            options: options ? {
                humanize: options.humanize ?? DEFAULTS.move.humanize,
                trajectory: options.trajectory ?? DEFAULTS.move.trajectory,
                duration: options.duration ?? DEFAULTS.move.duration,
            } : undefined,
        });
        return response.data;
    }
    
    async clickMouse(params: ClickParams): Promise<ClickResult> {
        const startTime = Date.now();
        this.logger.debug('POST /api/mouse/click', { 
            window: params.window,
            xpath: params.xpath.substring(0, 80) + '...' 
        });
        
        try {
            const response = await this.client.post<ClickResult>('/api/mouse/click', {
                window: params.window,
                xpath: params.xpath,
                options: params.options ? {
                    humanize: params.options.humanize ?? DEFAULTS.click.humanize,
                    randomRange: params.options.randomRange ?? DEFAULTS.click.randomRange,
                    pauseBefore: params.options.pauseBefore ?? DEFAULTS.click.pauseBefore,
                    pauseAfter: params.options.pauseAfter ?? DEFAULTS.click.pauseAfter,
                } : undefined,
            });
            
            const duration = Date.now() - startTime;
            this.logger.info('Click completed', { 
                duration, 
                success: response.data.success,
                clickPoint: response.data.success ? response.data.clickPoint : undefined
            });
            
            return response.data;
        } catch (error) {
            this.logger.error('Click failed', { params, error: (error as Error).message });
            throw this.handleError(error, '/api/mouse/click');
        }
    }
    
    async startIdleMotion(params: IdleMotionParams): Promise<void> {
        await this.client.post('/api/mouse/idle/start', {
            window: params.window,
            xpath: params.xpath,
            speed: params.speed ?? DEFAULTS.idleMotion.speed,
            moveInterval: params.moveInterval ?? DEFAULTS.idleMotion.moveInterval,
            idleTimeout: params.idleTimeout ?? DEFAULTS.idleMotion.idleTimeout,
            humanIntervention: params.humanIntervention ? {
                enabled: params.humanIntervention.enabled,
                pauseOnMouse: params.humanIntervention.pauseOnMouse ?? DEFAULTS.idleMotion.humanIntervention.pauseOnMouse,
                pauseOnKeyboard: params.humanIntervention.pauseOnKeyboard ?? DEFAULTS.idleMotion.humanIntervention.pauseOnKeyboard,
                resumeDelay: params.humanIntervention.resumeDelay ?? DEFAULTS.idleMotion.humanIntervention.resumeDelay,
            } : DEFAULTS.idleMotion.humanIntervention,
        });
    }
    
    async stopIdleMotion(): Promise<StopResult> {
        const response = await this.client.post<StopResult>('/api/mouse/idle/stop');
        return response.data;
    }
    
    async getIdleMotionStatus(): Promise<IdleMotionStatus> {
        const response = await this.client.get<IdleMotionStatus>('/api/mouse/idle/status');
        return response.data;
    }
    
    async typeText(text: string, options?: TypeOptions): Promise<TypeResult> {
        const response = await this.client.post<TypeResult>('/api/keyboard/type', {
            text,
            charDelay: options?.charDelay ?? DEFAULTS.type.charDelay,
        });
        return response.data;
    }
    
    /**
     * 激活指定窗口（使其成为前台窗口）
     * @param windowSelector 窗口选择器 XPath
     * @returns 激活结果
     */
    async activateWindow(windowSelector: string): Promise<{ success: boolean; error?: string }> {
        const response = await this.client.post<{ success: boolean; windowSelector: string; error?: string }>('/api/window/activate', {
            windowSelector,
        });
        return response.data;
    }
    
    /**
     * 激活窗口并使指定元素获得焦点
     * @param windowSelector 窗口选择器 XPath
     * @param xpath 元素 XPath
     * @returns 操作结果
     */
    async focusElement(windowSelector: string, xpath: string): Promise<{ success: boolean; error?: string }> {
        const response = await this.client.post<{ success: boolean; error?: string }>('/api/window/focus-element', {
            windowSelector,
            xpath,
        });
        return response.data;
    }
    
    /**
     * 获取所有匹配元素
     * @param params 查询参数
     * @returns 所有匹配的元素列表
     */
    async getAllElements(params: ElementQueryParams): Promise<{ found: boolean; elements: ElementInfo[]; total: number; error?: string }> {
        const response = await this.client.get<{ found: boolean; elements: ElementInfo[]; total: number; error?: string }>('/api/element/all', {
            params: {
                windowSelector: params.windowSelector,
                xpath: params.xpath,
                randomRange: params.randomRange ?? DEFAULTS.click.randomRange,
            },
        });
        return response.data;
    }
    
    /**
     * 执行快捷键组合
     * @param keys 快捷键字符串，如 "Ctrl+C", "Alt+F4"
     */
    async executeShortcut(keys: string): Promise<{ success: boolean; error?: string }> {
        const response = await this.client.post<{ success: boolean; error?: string }>('/api/keyboard/shortcut', {
            keys,
        });
        return response.data;
    }
    
    /**
     * 执行单个按键
     * @param key 按键名称，如 "Enter", "Tab", "Escape"
     */
    async executeKey(key: string): Promise<{ success: boolean; error?: string }> {
        const response = await this.client.post<{ success: boolean; error?: string }>('/api/keyboard/key', {
            key,
        });
        return response.data;
    }
    
    handleError(error: unknown, endpoint?: string): never {
        if (axios.isAxiosError(error)) {
            const axiosError = error as AxiosError<{ error?: string }>;
            
            // 超时错误
            if (axiosError.code === 'ECONNABORTED') {
                throw new TimeoutError(
                    endpoint || 'unknown', 
                    this.client.defaults.timeout || 30000
                );
            }
            
            // 网络错误（无响应）
            if (!axiosError.response) {
                throw new NetworkError(
                    axiosError, 
                    endpoint || 'unknown'
                );
            }
            
            // HTTP 错误
            const message = axiosError.response?.data?.error ?? axiosError.message;
            throw new SDKError(
                `HTTP ${axiosError.response.status}: ${message}`,
                `HTTP_${axiosError.response.status}`,
                { 
                    endpoint, 
                    status: axiosError.response.status,
                    responseData: axiosError.response.data
                }
            );
        }
        
        // 其他错误
        if (error instanceof Error) {
            throw new SDKError(
                error.message, 
                'UNKNOWN_ERROR', 
                { stack: error.stack }
            );
        }
        
        throw new SDKError(String(error), 'UNKNOWN_ERROR');
    }
}