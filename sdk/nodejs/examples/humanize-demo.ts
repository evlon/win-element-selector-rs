// sdk/nodejs/examples/humanize-demo.ts
// 拟人化操作示例 - 展示 humanize 上下文和链式调用

import { ElementSelectorSDK } from '../src';

async function main() {
    const sdk = new ElementSelectorSDK({
        baseUrl: 'http://127.0.0.1:8080',
    });

    try {
        // 1. 获取窗口列表，找到记事本
        console.log('=== 获取窗口列表 ===');
        const windows = await sdk.listWindows();
        const notepadWindow = windows.find(w => w.processName === 'Notepad');
        
        if (!notepadWindow) {
            console.log('请先打开一个记事本窗口！');
            return;
        }
        
        const windowSelector = {
            title: notepadWindow.title,
            className: notepadWindow.className,
            processName: notepadWindow.processName,
        };
        console.log('记事本窗口:', windowSelector.title);
        console.log();

        // 2. 先激活窗口
        console.log('=== 激活窗口 ===');
        await sdk.activateWindow(windowSelector);
        console.log('窗口已激活');
        console.log();

        console.log('=== 拟人化上下文示例 ===');

        // 使用 humanize 上下文自动应用拟人化参数
        await sdk.humanize(async (ctx) => {
            console.log('1. 在拟人上下文中点击...');
            
            // 所有操作自动应用默认拟人化参数
            await ctx.click({
                window: windowSelector,
                xpath: '//Document'
            });
            
            console.log('2. 打字（自动随机延迟）...');
            await ctx.type('拟人化输入测试');
            
            console.log('3. 查找元素...');
            const elem = await ctx.getElement({
                windowSelector: `Window[@Name='${notepadWindow.title}' and @ClassName='${notepadWindow.className}']`,
                xpath: '//Document'
            });
            console.log('元素状态:', elem.found ? '找到' : '未找到');
        });

        console.log();
        console.log('=== 链式调用示例 ===');

        // 使用链式调用执行一系列操作
        await sdk.humanize(async (ctx) => {
            await ctx.chain()
                .click({ window: windowSelector, xpath: '//Document' })
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