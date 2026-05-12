// 让我们点击一下元宝聊天窗口中的按钮

import { SDK } from '../src';

async function main() {
    const sdk = new SDK();

  try {
        console.log('=== Element Selector SDK 流式调用示例 ===');
        console.log();

        // 1. 健康检查
        console.log('1. 健康检查...');
        const health = await sdk.health();
        console.log(`   服务状态: ${health.status}`);
        console.log();
           // 2. 获取窗口列表
        console.log('2. 获取窗口列表...');
        const windows = await sdk.listWindows();
        const yuanbaoWindow = windows.find(w => w.processName === 'yuanbao');     
        if (yuanbaoWindow) {
            console.log(`   找到元宝窗口: ${yuanbaoWindow.title}`);
        } else {
            console.log('   没有找到元宝窗口');
            return;
        }

        console.log('3. 切换到元宝窗口...');
        await sdk.flow()
            .window({
                title: yuanbaoWindow.title,
                processName: yuanbaoWindow.processName,
            })
            .humanize()
            .find(`//Group[contains(@ClassName, 'temp-dialogue-btn')]`)
            .click()
            .wait(500)
            .run();
    } catch (error) {
        console.error('发生错误:', error);
    }
}
main();