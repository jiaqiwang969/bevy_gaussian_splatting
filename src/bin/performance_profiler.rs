// 3DGS 性能分析工具
// 类似摄像头项目的 performance_profiler，实时监控各环节性能

use bevy::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_gaussian_splatting::{GaussianCamera, GaussianSplattingPlugin, PlanarGaussian3dHandle, CloudSettings};

#[derive(Resource)]
struct PerformanceStats {
    frame_times: Vec<f32>,
    max_samples: usize,
}

impl Default for PerformanceStats {
    fn default() -> Self {
        Self {
            frame_times: Vec::new(),
            max_samples: 300, // 5秒 @ 60fps
        }
    }
}

fn main() {
    println!("=== 3DGS 性能分析工具 ===\n");
    println!("实时监控渲染性能，识别瓶颈\n");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "3DGS Performance Profiler".to_string(),
                resolution: (1280, 720).into(),
                present_mode: bevy::window::PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GaussianSplattingPlugin)
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(PerformanceStats::default())
        .add_systems(Startup, setup)
        .add_systems(Update, (monitor_performance, display_stats))
        .run();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    println!("📊 开始性能分析...\n");

    // 加载测试 PLY
    commands.spawn((
        PlanarGaussian3dHandle(asset_server.load("bevy_logo.ply")),
        CloudSettings::default(),
        Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
        Visibility::default(),
    ));

    // 相机
    commands.spawn((
        GaussianCamera { warmup: true },
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // 光源
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.5, 0.0)),
    ));

    // UI 文本
    commands.spawn((
        Text::new("Performance Stats"),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::srgb(0.0, 1.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

fn monitor_performance(
    mut stats: ResMut<PerformanceStats>,
    diagnostics: Res<DiagnosticsStore>,
) {
    if let Some(fps_diag) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
        if let Some(fps) = fps_diag.smoothed() {
            let frame_time = 1000.0 / fps as f32;
            stats.frame_times.push(frame_time);

            // 保持固定样本数
            if stats.frame_times.len() > stats.max_samples {
                stats.frame_times.remove(0);
            }
        }
    }
}

fn display_stats(
    stats: Res<PerformanceStats>,
    diagnostics: Res<DiagnosticsStore>,
    mut query: Query<&mut Text>,
    time: Res<Time>,
) {
    // 每秒更新一次显示
    if time.elapsed_secs() % 1.0 > 0.5 {
        return;
    }

    if stats.frame_times.is_empty() {
        return;
    }

    // 计算统计数据
    let avg_frame_time = stats.frame_times.iter().sum::<f32>() / stats.frame_times.len() as f32;
    let min_frame_time = stats.frame_times.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_frame_time = stats.frame_times.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    let avg_fps = 1000.0 / avg_frame_time;
    let min_fps = 1000.0 / max_frame_time;
    let max_fps = 1000.0 / min_frame_time;

    // 获取 GPU 信息（如果可用）
    let mut gpu_info = String::from("N/A");
    if let Some(frame_time_diag) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME) {
        if let Some(frame_time) = frame_time_diag.smoothed() {
            gpu_info = format!("{:.2}ms", frame_time * 1000.0);
        }
    }

    // 性能评估
    let performance_rating = if avg_fps >= 55.0 {
        "✓ 优秀"
    } else if avg_fps >= 30.0 {
        "⚠ 良好"
    } else {
        "✗ 需优化"
    };

    // 瓶颈分析
    let bottleneck = if max_frame_time > 33.0 {
        "⚠️ 检测到帧时间峰值 (>33ms)"
    } else if avg_frame_time > 16.67 {
        "⚠️ 平均帧时间偏高"
    } else {
        "✓ 无明显瓶颈"
    };

    // 更新显示
    for mut text in query.iter_mut() {
        **text = format!(
            "=== 3DGS 性能分析 ===\n\
            \n\
            帧率 (FPS):\n\
              平均: {:.1} fps\n\
              最小: {:.1} fps\n\
              最大: {:.1} fps\n\
            \n\
            帧时间 (ms):\n\
              平均: {:.2} ms\n\
              最小: {:.2} ms\n\
              最大: {:.2} ms\n\
            \n\
            GPU 帧时间: {}\n\
            \n\
            性能评级: {}\n\
            瓶颈分析: {}\n\
            \n\
            样本数: {} 帧\n\
            \n\
            对比摄像头项目:\n\
            - 摄像头解码: 0.91ms (优化后)\n\
            - 3DGS 渲染: {:.2}ms (当前)\n\
            \n\
            优化建议:\n\
            {}",
            avg_fps,
            min_fps,
            max_fps,
            avg_frame_time,
            min_frame_time,
            max_frame_time,
            gpu_info,
            performance_rating,
            bottleneck,
            stats.frame_times.len(),
            avg_frame_time,
            get_optimization_suggestions(avg_frame_time, max_frame_time)
        );
    }

    // 控制台输出（每5秒）
    if time.elapsed_secs() % 5.0 < 1.0 {
        println!("\n📊 性能报告 ({:.0}秒):", time.elapsed_secs());
        println!("  平均 FPS: {:.1}", avg_fps);
        println!("  平均帧时间: {:.2}ms", avg_frame_time);
        println!("  性能评级: {}", performance_rating);
        println!("  {}", bottleneck);
    }
}

fn get_optimization_suggestions(avg_frame_time: f32, max_frame_time: f32) -> String {
    let mut suggestions = Vec::new();

    if avg_frame_time > 16.67 {
        suggestions.push("• 考虑降低点云密度");
    }

    if max_frame_time > 33.0 {
        suggestions.push("• 实现视锥体剔除");
    }

    if avg_frame_time < 10.0 {
        suggestions.push("• 性能充足，可提升画质");
    }

    if suggestions.is_empty() {
        suggestions.push("• 性能良好，无需优化");
    }

    suggestions.join("\n")
}
