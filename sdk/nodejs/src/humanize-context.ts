import { HttpClient } from './client';
import {
    ClickParams,
    ClickResult,
    MoveParams,
    MoveResult,
    TypeOptions,
    TypeResult,
    ElementQueryParams,
    ElementResponse,
    DEFAULTS,
} from './types';
import { ActionChain } from './action-chain';

export class HumanizeContext {
    constructor(private client: HttpClient) {}
    
    async click(params: ClickParams): Promise<ClickResult> {
        const mergedParams: ClickParams = {
            ...params,
            options: {
                humanize: params.options?.humanize ?? DEFAULTS.click.humanize,
                randomRange: params.options?.randomRange ?? DEFAULTS.click.randomRange,
                pauseBefore: params.options?.pauseBefore ?? DEFAULTS.click.pauseBefore,
                pauseAfter: params.options?.pauseAfter ?? DEFAULTS.click.pauseAfter,
            },
        };
        return this.client.clickMouse(mergedParams);
    }
    
    async move(params: MoveParams): Promise<MoveResult> {
        const mergedParams: MoveParams = {
            target: params.target,
            options: {
                humanize: params.options?.humanize ?? DEFAULTS.move.humanize,
                trajectory: params.options?.trajectory ?? DEFAULTS.move.trajectory,
                duration: params.options?.duration ?? DEFAULTS.move.duration,
            },
        };
        return this.client.moveMouse(mergedParams.target, mergedParams.options);
    }
    
    async type(text: string, options?: TypeOptions): Promise<TypeResult> {
        return this.client.typeText(text, options);
    }
    
    async getElement(params: ElementQueryParams): Promise<ElementResponse> {
        return this.client.getElement(params);
    }
    
    chain(): ActionChain {
        return new ActionChain(this);
    }
}