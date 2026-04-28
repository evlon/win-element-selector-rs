// sdk/nodejs/examples/v2-basic-usage.ts
// SDK V2 流式调用示例

import { ElementSelectorSDKv2 } from '../src/v2';

async function main() {
    const sdk = new ElementSelectorSDKv2();

    try {
        console.log('=== SDK V2 流式调用示例 ===');
        console.log();

        // 1. 获取窗口列表
        console.log('获取窗口列表...');
        const windows = await sdk.listWindows();
        const notepad = windows.find(w => w.processName === 'Notepad');
        
        if (!notepad) {
            console.log('请先打开一个记事本窗口！');
            return;
        }
        console.log(`找到记事本: ${notepad.title}`);
        console.log();

        // 2. 流式调用 - 简单示例
        console.log('=== 流式调用示例 ===');
        await sdk
            .humanize()                               // 开启拟人化
            .debug()                                  // 开启调试日志
            .window({
                title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')                       // 查找编辑区域
            .click()                                  // 点击
            .type('SDK V2 测试内容')                   // 打字
            .wait(500, 1000)                          // 随机等待 500-1000ms
            .run();                                   // 执行
        
        console.log('流式调用完成！');
        console.log();

        // 3. 流式调用 - 链式操作
        console.log('=== 链式操作示例 ===');
        await sdk
            .chain()                                  // 创建链
            .humanize({ speed: 'slow' })              // 慢速拟人化
            .window({
                title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')
            .click()
            .type('\n第二行内容')
            .wait(300)
            .type('\n第三行内容')
            .run();
        
        console.log('链式操作完成！');
        console.log();

        // 4. 元素信息查询
        console.log('=== 元素信息查询 ===');
        const info = await sdk
            .window({
                title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')
            .inspect();
        
        if (info) {
            console.log('元素信息:', {
                name: info.name,
                controlType: info.controlType,
                rect: info.rect,
                center: info.center,
            });
        }
        console.log();

    } catch (error) {
        console.error('错误:', error);
    }
}

main().catch(console.error);