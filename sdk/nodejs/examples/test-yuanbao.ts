// 让我们点击一下元宝聊天窗口中的按钮

import { SDK, ElementNotFoundError } from '../src';

async function main() {
    const sdk = new SDK();

    try {
        console.log('=== Element Selector SDK 流式调用示例 ===\n');

        // 1. 健康检查
        console.log('1. 健康检查...');
        const health = await sdk.health();
        console.log(`   服务状态: ${health.status}\n`);

        // 执行自动化流程
        await sdk.flow()
            .humanize()
            .window({ title: '元宝', className: 'Tauri Window', processName: 'yuanbao' })
            .find(`//Document[@ControlType='Document' and @AutomationId='RootWebArea' and @FrameworkId='Chrome' and @LocalizedControlType='文档']/Group[@ControlType='Group' and @FrameworkId='Chrome' and @LocalizedControlType='组']/Group[@ControlType='Group' and starts-with(@ClassName, 'chat_mainPage__wilLn') and @FrameworkId='Chrome' and @LocalizedControlType='组']/Group[@ControlType='Group' and starts-with(@ClassName, 'temp-dialogue-btn_temp-dialogue') and @FrameworkId='Chrome' and @LocalizedControlType='组']`)
            .wait(500)
            .click()
            .wait(3000)  // 点击后等待 3 秒，给页面足够时间加载新内容
            .find('//Document[@ControlType=\'Document\' and @AutomationId=\'RootWebArea\' and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'文档\']/Group[@ControlType=\'Group\' and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']/Group[@ControlType=\'Group\' and starts-with(@ClassName, \'chat_mainPage__wilLn\') and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']/Group[@ControlType=\'Group\' and starts-with(@ClassName, \'chat_chat\') and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']/Group[@ControlType=\'Group\' and starts-with(@ClassName, \'index_v2_search\') and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']/Group[@ControlType=\'Group\' and starts-with(@ClassName, \'chat-command-editor-specail\') and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']/Group[@ControlType=\'Group\' and @ClassName=\'ql-editor ql-blank\' and @FrameworkId=\'Chrome\' and @LocalizedControlType=\'组\']')
            .click()
            .type('测试')
            .run();

        console.log('\n✅ 所有操作成功完成');
    } catch (error) {
        if (error instanceof ElementNotFoundError) {
            console.error('\n❌ 元素未找到');
            console.error(`   XPath: ${error.context?.xpath}`);
            console.error(`   窗口: ${error.context?.windowSelector}`);
            console.error(`   截图: ${error.context?.screenshotPath}`);
            console.error(`   提示: ${error.context?.hint}`);
        } else if (error instanceof Error) {
            console.error('\n❌ 发生错误:', error.message);
            if (process.env.LOG_LEVEL === 'debug') {
                console.error(error.stack);
            }
        }
        process.exit(1);
    }
}
main();