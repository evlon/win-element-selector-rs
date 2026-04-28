import axios, { AxiosInstance, AxiosError } from 'axios';
import {
    SDKConfig,
    DEFAULTS,
    HealthStatus,
    WindowInfo,
    ElementQueryParams,
    ElementResponse,
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

export class HttpClient {
    private client: AxiosInstance;
    
    constructor(config: SDKConfig) {
        this.client = axios.create({
            baseURL: config.baseUrl,
            timeout: config.timeout ?? DEFAULTS.timeout,
            headers: {
                'Content-Type': 'application/json',
            },
        });
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
        const response = await this.client.get<ElementResponse>('/api/element', {
            params: {
                windowSelector: params.windowSelector,
                xpath: params.xpath,
                randomRange: params.randomRange ?? DEFAULTS.click.randomRange,
            },
        });
        return response.data;
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
        return response.data;
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
    
    handleError(error: unknown): Error {
        if (axios.isAxiosError(error)) {
            const axiosError = error as AxiosError<{ error?: string }>;
            const message = axiosError.response?.data?.error 
                ?? axiosError.message 
                ?? 'Unknown HTTP error';
            return new Error(`SDK Error: ${message}`);
        }
        return error instanceof Error ? error : new Error(String(error));
    }
}