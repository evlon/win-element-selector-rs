// src/mouse_control.rs
//
// 鼠标拟人化控制模块 - 使用 SendInput API 模拟鼠标操作
// 实现贝塞尔曲线轨迹、缓动函数、拟人化移动

use log::{debug, info};
use std::time::{Duration, Instant};
use rand::Rng;

use windows::Win32::{
    Foundation::POINT,
    UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSE_EVENT_FLAGS,
    },
    UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

// ═══════════════════════════════════════════════════════════════════════════════
// 点和轨迹
// ═══════════════════════════════════════════════════════════════════════════════

/// 2D 点
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 公共 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 获取当前鼠标位置
pub fn get_cursor_position() -> super::api::types::Point {
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            super::api::types::Point::new(pt.x, pt.y)
        } else {
            super::api::types::Point::new(0, 0)
        }
    }
}

/// 直线移动鼠标（非拟人化）
pub fn linear_move(start: super::api::types::Point, end: super::api::types::Point) -> anyhow::Result<()> {
    set_cursor_position(end.x, end.y);
    info!("Linear move: ({}, {}) -> ({}, {})", start.x, start.y, end.x, end.y);
    Ok(())
}

/// 拟人化移动鼠标（贝塞尔曲线轨迹）
pub fn humanized_move(
    start: super::api::types::Point,
    end: super::api::types::Point,
    duration_ms: u64,
    trajectory_type: &str,
) -> anyhow::Result<()> {
    info!(
        "Humanized move: ({}, {}) -> ({}, {}) duration={}ms trajectory={}",
        start.x, start.y, end.x, end.y, duration_ms, trajectory_type
    );
    
    match trajectory_type {
        "bezier" => bezier_move(start, end, duration_ms),
        "linear" => linear_move_with_easing(start, end, duration_ms),
        _ => bezier_move(start, end, duration_ms), // 默认贝塞尔
    }
}

/// 在指定位置执行左键点击
pub fn click_at(point: super::api::types::Point) -> anyhow::Result<()> {
    // 先移动到目标位置
    set_cursor_position(point.x, point.y);
    
    // 短暂停顿模拟真实点击
    std::thread::sleep(Duration::from_millis(50));
    
    // 模拟按下
    send_mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0);
    
    // 按下持续时间（50-100ms 随机）
    let mut rng = rand::thread_rng();
    let press_duration = rng.gen_range(50u64..100u64);
    std::thread::sleep(Duration::from_millis(press_duration));
    
    // 模拟释放
    send_mouse_event(MOUSEEVENTF_LEFTUP, 0, 0);
    
    info!("Click executed at ({}, {})", point.x, point.y);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// 贝塞尔曲线轨迹
// ═══════════════════════════════════════════════════════════════════════════════

/// 贝塞尔曲线移动
fn bezier_move(start: super::api::types::Point, end: super::api::types::Point, duration_ms: u64) -> anyhow::Result<()> {
    // 生成贝塞尔控制点（带随机扰动）
    let control_points = generate_bezier_control_points(start, end);
    
    // 生成轨迹点（带缓动）
    let trajectory = generate_bezier_trajectory(&control_points, duration_ms);
    
    // 执行轨迹移动
    execute_trajectory(&trajectory);
    
    Ok(())
}

/// 生成三次贝塞尔曲线控制点（4个点：P0, P1, P2, P3）
fn generate_bezier_control_points(start: super::api::types::Point, end: super::api::types::Point) -> [Point; 4] {
    use rand::Rng;
    
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    
    // 控制点扰动范围：10-30% 的路径长度
    let mut rng = rand::thread_rng();
    let perturb_range = 0.3;
    
    // P1 在起点附近，向前偏移
    let p1_x = start.x + (dx * 0.25 + rng.gen_range(-dx.abs() * perturb_range..dx.abs() * perturb_range)) as i32;
    let p1_y = start.y + (dy * 0.25 + rng.gen_range(-dy.abs() * perturb_range..dy.abs() * perturb_range)) as i32;
    
    // P2 在终点附近，向后偏移
    let p2_x = start.x + (dx * 0.75 + rng.gen_range(-dx.abs() * perturb_range..dx.abs() * perturb_range)) as i32;
    let p2_y = start.y + (dy * 0.75 + rng.gen_range(-dy.abs() * perturb_range..dy.abs() * perturb_range)) as i32;
    
    [
        Point::new(start.x, start.y),  // P0: 起点
        Point::new(p1_x, p1_y),        // P1: 第一个控制点
        Point::new(p2_x, p2_y),        // P2: 第二个控制点
        Point::new(end.x, end.y),      // P3: 终点
    ]
}

/// 计算三次贝塞尔曲线上的点
fn cubic_bezier(p: &[Point; 4], t: f32) -> Point {
    // B(t) = (1-t)^3 * P0 + 3*(1-t)^2*t * P1 + 3*(1-t)*t^2 * P2 + t^3 * P3
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;
    
    let x = mt3 * p[0].x as f32 + 3.0 * mt2 * t * p[1].x as f32 + 3.0 * mt * t2 * p[2].x as f32 + t3 * p[3].x as f32;
    let y = mt3 * p[0].y as f32 + 3.0 * mt2 * t * p[1].y as f32 + 3.0 * mt * t2 * p[2].y as f32 + t3 * p[3].y as f32;
    
    Point::new(x as i32, y as i32)
}

/// 生成贝塞尔轨迹点（带 ease-in-out 缓动）
fn generate_bezier_trajectory(control_points: &[Point; 4], duration_ms: u64) -> Vec<(Point, Duration)> {
    // 轨迹采样点数（根据时长调整）
    let num_points = std::cmp::max(50, (duration_ms / 10) as usize);
    
    let mut trajectory: Vec<(Point, Duration)> = Vec::with_capacity(num_points);
    
    let step_duration = Duration::from_millis(duration_ms / num_points as u64);
    
    for i in 0..num_points {
        // 应用 ease-in-out 缓动
        let t_raw = i as f32 / num_points as f32;
        let t = ease_in_out(t_raw);
        
        let point = cubic_bezier(control_points, t);
        trajectory.push((point, step_duration));
    }
    
    // 确保最后一点是终点
    if let Some(last) = trajectory.last_mut() {
        last.0 = control_points[3];
    }
    
    trajectory
}

// ═══════════════════════════════════════════════════════════════════════════════
// 缓动函数
// ═══════════════════════════════════════════════════════════════════════════════

/// Ease-in-out 缓动函数
/// 启动时加速，到达前减速
fn ease_in_out(t: f32) -> f32 {
    // Sinusoidal ease-in-out: 平滑的加速/减速曲线
    // f(t) = -(cos(pi*t) - 1) / 2
    -(std::f32::consts::PI * t).cos() / 2.0 + 0.5
}

/// 直线移动带缓动
fn linear_move_with_easing(start: super::api::types::Point, end: super::api::types::Point, duration_ms: u64) -> anyhow::Result<()> {
    let num_points = std::cmp::max(30, (duration_ms / 20) as usize);
    let step_duration = Duration::from_millis(duration_ms / num_points as u64);
    
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    
    let mut trajectory: Vec<(Point, Duration)> = Vec::with_capacity(num_points);
    
    for i in 0..num_points {
        let t_raw = i as f32 / num_points as f32;
        let t = ease_in_out(t_raw);
        
        let x = start.x + (dx * t) as i32;
        let y = start.y + (dy * t) as i32;
        
        trajectory.push((Point::new(x, y), step_duration));
    }
    
    // 确保最后一点是终点
    if let Some(last) = trajectory.last_mut() {
        last.0 = Point::new(end.x, end.y);
    }
    
    execute_trajectory(&trajectory);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Windows API 实现
// ═══════════════════════════════════════════════════════════════════════════════

fn execute_trajectory(trajectory: &[(Point, Duration)]) {
    let start_time = Instant::now();
    
    for (point, step_duration) in trajectory {
        set_cursor_position(point.x, point.y);
        
        // 精确控制间隔
        let elapsed = start_time.elapsed();
        let target_elapsed = elapsed + *step_duration;
        
        if elapsed < target_elapsed {
            std::thread::sleep(target_elapsed - elapsed);
        }
        
        debug!("Move to ({}, {}) elapsed={}ms", point.x, point.y, elapsed.as_millis());
    }
}

fn set_cursor_position(x: i32, y: i32) {
    unsafe {
        // 获取屏幕尺寸用于绝对坐标转换
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        
        // 将屏幕坐标转换为 0-65535 范围
        let normalized_x = (x * 65536) / screen_width;
        let normalized_y = (y * 65536) / screen_height;
        
        send_mouse_event(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, normalized_x, normalized_y);
    }
}

fn send_mouse_event(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32) {
    unsafe {
        let mouse_input = MOUSEINPUT {
            dx: dx,
            dy: dy,
            mouseData: 0,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
        
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: mouse_input },
        };
        
        let inputs = [input];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}