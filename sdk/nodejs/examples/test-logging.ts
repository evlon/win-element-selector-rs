// 测试日志和错误处理功能

import { SDK, LogConfig, ElementNotFoundError } from '../src';

async function testLogging() {
    console.log('=== 日志系统测试 ===\n');

    // 测试 1: 不同日志级别
    console.log('测试 1: 设置日志级别为 debug\n');
    LogConfig.setLevel('debug');
    
    const sdk = new SDK();
    
    try {
        console.log('执行健康检查...\n');
        const health = await sdk.health();
        console.log(`✓ 服务状态: ${health.status}\n`);
        
        // 测试 2: 尝试查找不存在的元素（会触发错误日志）
        console.log('测试 2: 尝试查找不存在的窗口（预期失败）\n');
        
        await sdk.flow()
            .window({ title: 'NonExistentWindow_12345' })
            .find('//Button')
            .run();
            
    } catch (error) {
        if (error instanceof ElementNotFoundError) {
            console.log('\n✓ 捕获到 ElementNotFoundError');
            console.log(`  XPath: ${error.context?.xpath}`);
            console.log(`  窗口: ${error.context?.windowSelector}`);
            console.log(`  截图: ${error.context?.screenshotPath}`);
        } else if (error instanceof Error) {
            console.log('\n✓ 捕获到其他错误');
            console.log(`  消息: ${error.message}`);
        }
    }
    
    console.log('\n=== 测试完成 ===');
}

testLogging().catch(console.error);
