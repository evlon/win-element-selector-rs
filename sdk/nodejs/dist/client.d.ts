import { SDKConfig, HealthStatus, WindowInfo, ElementQueryParams, ElementResponse, MoveResult, ClickParams, ClickResult, IdleMotionParams, IdleMotionStatus, StopResult, Point, MoveOptions } from './types';
export declare class HttpClient {
    private client;
    constructor(config: SDKConfig);
    health(): Promise<HealthStatus>;
    listWindows(): Promise<WindowInfo[]>;
    getElement(params: ElementQueryParams): Promise<ElementResponse>;
    moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult>;
    clickMouse(params: ClickParams): Promise<ClickResult>;
    startIdleMotion(params: IdleMotionParams): Promise<void>;
    stopIdleMotion(): Promise<StopResult>;
    getIdleMotionStatus(): Promise<IdleMotionStatus>;
    handleError(error: unknown): Error;
}
//# sourceMappingURL=client.d.ts.map