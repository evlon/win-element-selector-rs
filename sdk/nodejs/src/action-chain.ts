import { HumanizeContext } from './humanize-context';
import { ClickParams, MoveParams, TypeOptions, Point } from './types';

interface ChainAction {
    type: 'click' | 'type' | 'move' | 'wait';
    params?: unknown;
    text?: string;
    options?: unknown;
    duration?: number;
}

export class ActionChain {
    private actions: ChainAction[] = [];
    
    constructor(private context: HumanizeContext) {}
    
    click(params: ClickParams): this {
        this.actions.push({ type: 'click', params });
        return this;
    }
    
    type(text: string, options?: TypeOptions): this {
        this.actions.push({ type: 'type', text, options });
        return this;
    }
    
    move(target: Point, options?: MoveParams['options']): this {
        this.actions.push({ type: 'move', params: { target, options } });
        return this;
    }
    
    wait(ms: number): this {
        this.actions.push({ type: 'wait', duration: ms });
        return this;
    }
    
    async execute(): Promise<void> {
        for (const action of this.actions) {
            switch (action.type) {
                case 'click':
                    await this.context.click(action.params as ClickParams);
                    break;
                case 'type':
                    await this.context.type(action.text!, action.options as TypeOptions);
                    break;
                case 'move':
                    const moveParams = action.params as MoveParams;
                    await this.context.move({
                        target: moveParams.target,
                        options: moveParams.options,
                    });
                    break;
                case 'wait':
                    await new Promise(resolve => setTimeout(resolve, action.duration!));
                    break;
            }
        }
    }
}