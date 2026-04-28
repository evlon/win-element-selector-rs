// sdk/nodejs/examples/humanize-demo.ts
// 拟人化操作示例 - 展示 humanize 上下文和链式调用

import { ElementSelectorSDK } from '../src';

async function main() {
    const sdk = new ElementSelectorSDK({
        baseUrl: 'http://127.0.0.1:8080',
    });

    try {
        console.log('=== 拟人化上下文示例 ===\n');

        // 使用 humanize 上下文自动应用拟人化参数
        await sdk.humanize(async (ctx) => {
            console.log('1. 在拟人上下文中点击...');
            
            // 所有操作自动应用默认拟人化参数
            await ctx.click({
                window: { className: 'Notepad' },
                xpath: '//Edit'
            });
            
            console.log('2. 打字（自动随机延迟）...');
            await ctx.type('拟人化输入测试');
            
            console.log('3. 查找元素...');
            const elem = await ctx.getElement({
                windowSelector: "Window[@ClassName='Notepad']",
                xpath: '//Edit'
            });
            console.log('元素状态:', elem.found ? '找到' : '未找到');
        });

        console.log('\n=== 链式调用示例 ===\n');

        // 使用链式调用执行一系列操作
        await sdk.humanize(async (ctx) => {
            await ctx.chain()
                .click({ window: { className: 'Notepad' }, xpath: '//Edit' })
                .wait(200)
                .type('链式调用测试 - 第一步')
                .wait(300)
                .type('\n第二步内容')
                .wait(200)
                .move({ x: 500, y: 300 })
                .execute();
            
            console.log('链式调用完成！');
        });

    } catch (error) {
        console.error('执行出错:', error);
    }
}

main().catch(console.error);