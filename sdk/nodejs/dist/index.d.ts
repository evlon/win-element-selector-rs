import { HumanizeContext } from './humanize-context';
import { SDKConfig, HealthStatus, WindowInfo, WindowSelector, ElementQueryParams, ElementResponse, Point, MoveOptions, MoveResult, ClickParams, ClickResult, TypeOptions, TypeResult, IdleMotionParams, IdleMotionStatus, StopResult } from './types';
export declare class ElementSelectorSDK {
    private client;
    constructor(config?: Partial<SDKConfig>);
    health(): Promise<HealthStatus>;
    listWindows(): Promise<WindowInfo[]>;
    getElement(params: ElementQueryParams): Promise<ElementResponse>;
    moveMouse(target: Point, options?: MoveOptions): Promise<MoveResult>;
    click(params: ClickParams): Promise<ClickResult>;
    type(text: string, options?: TypeOptions): Promise<TypeResult>;
    humanize<T>(callback: (ctx: HumanizeContext) => Promise<T>): Promise<T>;
    startIdleMotion(params: IdleMotionParams): Promise<void>;
    stopIdleMotion(): Promise<StopResult>;
    getIdleMotionStatus(): Promise<IdleMotionStatus>;
    static buildWindowSelector(selector: WindowSelector): string;
}
export * from './types';
export { HumanizeContext } from './humanize-context';
export { ActionChain } from './action-chain';
export { HttpClient } from './client';
export { buildWindowSelector, sleep, randomInt, randomFloat } from './utils';
export default ElementSelectorSDK;
//# sourceMappingURL=index.d.ts.map