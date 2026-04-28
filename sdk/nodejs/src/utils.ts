export function sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
}

export function randomInt(min: number, max: number): number {
    return Math.floor(Math.random() * (max - min + 1)) + min;
}

export function randomFloat(min: number, max: number): number {
    return Math.random() * (max - min) + min;
}

export function buildWindowSelector(selector: {
    title?: string;
    className?: string;
    processName?: string;
}): string {
    const predicates: string[] = [];
    
    if (selector.title) {
        predicates.push(`@Name='${selector.title}'`);
    }
    if (selector.className) {
        predicates.push(`@ClassName='${selector.className}'`);
    }
    if (selector.processName) {
        predicates.push(`@ProcessName='${selector.processName}'`);
    }
    
    if (predicates.length === 0) {
        return 'Window';
    }
    return `Window[${predicates.join(' and ')}]`;
}