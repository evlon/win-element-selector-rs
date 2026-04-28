import { HttpClient } from './client';
import { ClickParams, ClickResult, MoveParams, MoveResult, TypeOptions, TypeResult, ElementQueryParams, ElementResponse } from './types';
import { ActionChain } from './action-chain';
export declare class HumanizeContext {
    private client;
    constructor(client: HttpClient);
    click(params: ClickParams): Promise<ClickResult>;
    move(params: MoveParams): Promise<MoveResult>;
    type(text: string, options?: TypeOptions): Promise<TypeResult>;
    getElement(params: ElementQueryParams): Promise<ElementResponse>;
    chain(): ActionChain;
}
//# sourceMappingURL=humanize-context.d.ts.map