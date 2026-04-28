import { HumanizeContext } from './humanize-context';
import { ClickParams, MoveParams, TypeOptions, Point } from './types';
export declare class ActionChain {
    private context;
    private actions;
    constructor(context: HumanizeContext);
    click(params: ClickParams): this;
    type(text: string, options?: TypeOptions): this;
    move(target: Point, options?: MoveParams['options']): this;
    wait(ms: number): this;
    execute(): Promise<void>;
}
//# sourceMappingURL=action-chain.d.ts.map