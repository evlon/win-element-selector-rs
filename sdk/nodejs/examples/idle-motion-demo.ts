// sdk/nodejs/examples/idle-motion-demo.ts
// 空闲移动示例 - 在元素区域内随机移动鼠标

import { ElementSelectorSDK } from '../src';

async function main() {
    const sdk = new ElementSelectorSDK();

    try {
        // 1. 检查当前状态
        console.log('=== 检查空闲移动状态 ===');
        const status = await sdk.getIdleMotionStatus();
        console.log('当前状态:', status);
        console.log();

        // 2. 启动空闲移动
        console.log('=== 启动空闲移动 ===');
        console.log('将在记事本窗口的编辑区域内随机移动鼠标...\n');
        
        await sdk.startIdleMotion({
            window: { className: 'Notepad' },
            xpath: '//Pane',  // 主面板区域
            
            speed: 'normal',
            moveInterval: 800,      // 每 800ms 移动一次
            idleTimeout: 30000,     // 30秒无操作自动停止
            
            // 人工干预检测
            humanIntervention: {
                enabled: true,
                pauseOnMouse: true,      // 用户移动鼠标时暂停
                pauseOnKeyboard: true,   // 用户按键时暂停
                resumeDelay: 3000        // 用户静止 3 秒后恢复
            }
        });
        
        console.log('空闲移动已启动！');
        console.log('提示：移动鼠标或按键会自动暂停，静止后恢复');
        console.log();

        // 3. 监控状态变化
        console.log('=== 监控状态（10秒）===');
        for (let i = 0; i < 10; i++) {
            await new Promise(r => setTimeout(r, 1000));
            const currentStatus = await sdk.getIdleMotionStatus();
            console.log(`[${i + 1}s] 活跃: ${currentStatus.active}, 暂停: ${currentStatus.paused}`);
            
            if (currentStatus.pauseReason) {
                console.log(`    暂停原因: ${currentStatus.pauseReason}`);
            }
        }
        console.log();

        // 4. 执行 API 操作（会自动暂停空闲移动）
        console.log('=== 执行操作（自动暂停空闲移动）===');
        await sdk.click({
            window: { className: 'Notepad' },
            xpath: '//Edit'
        });
        console.log('点击完成，空闲移动已恢复');
        console.log();

        // 5. 获取最终状态
        const finalStatus = await sdk.getIdleMotionStatus();
        console.log('最终状态:', {
            active: finalStatus.active,
            paused: finalStatus.paused,
            duration: finalStatus.runningDurationMs
        });
        console.log();

        // 6. 停止空闲移动
        console.log('=== 停止空闲移动 ===');
        const stopResult = await sdk.stopIdleMotion();
        console.log(`已停止，运行时长: ${stopResult.durationMs}ms`);
        console.log();

    } catch (error) {
        console.error('错误:', error);
        
        // 确保停止
        try {
            await sdk.stopIdleMotion();
        } catch {}
    }
}

main().catch(console.error);