# 🚀 快速启动指南

## 一键启动

```bash
./start_viewer.sh
```

## 手动启动

### 1. 启动服务器（在服务器上）

```bash
ssh wjq@192.168.31.164
cd /home/wjq/ml-sharp
./venv/bin/python server_simple.py
```

服务器将在 `http://192.168.31.164:8000` 启动

### 2. 启动客户端（在本地Mac）

```bash
cd /Users/jqwang/144-显微镜拍照-bevy-3dgs/microscope_viewer
./target/release/microscope_viewer
```

## 🎮 控制说明

| 按键 | 功能 |
|------|------|
| W | 向前移动 |
| S | 向后移动 |
| A | 向左移动 |
| D | 向右移动 |
| Space | 向上移动 |
| Shift | 向下移动 |

## 🔧 测试API

### 上传图片
```bash
curl -X POST -F "image=@your_image.jpg" http://192.168.31.164:8000/api/predict
```

### 下载PLY
```bash
curl -o result.ply http://192.168.31.164:8000/api/download/test
```

### 检查服务器状态
```bash
curl http://192.168.31.164:8000/
```

## 📊 性能指标

- **图片上传**: 0.1-0.5秒
- **PLY下载**: 2秒 (63MB)
- **渲染帧率**: 60 FPS
- **GPU**: Apple M4 Max (Metal)

## 🐛 故障排除

### 问题1: 看不到3DGS内容

**解决方案**:
```bash
# 确保PLY文件在正确位置
mkdir -p target/release/assets
cp assets/test.ply target/release/assets/
```

### 问题2: 服务器连接失败

**检查**:
```bash
# 测试服务器连接
curl http://192.168.31.164:8000/

# 如果失败，重启服务器
ssh wjq@192.168.31.164
cd /home/wjq/ml-sharp
./venv/bin/python server_simple.py
```

### 问题3: 编译错误

**解决方案**:
```bash
# 确保使用nightly Rust
rustup default nightly

# 清理并重新编译
cargo clean
cargo +nightly build --release
```

## 📁 项目结构

```
microscope_viewer/
├── src/
│   └── main.rs              # 主程序
├── assets/
│   └── test.ply             # 测试PLY文件 (63MB)
├── target/release/
│   ├── microscope_viewer    # 可执行文件
│   └── assets/
│       └── test.ply         # 运行时PLY文件
├── Cargo.toml               # Rust依赖配置
├── start_viewer.sh          # 启动脚本
├── README.md                # 项目说明
└── QUICKSTART.md            # 本文件
```

## 🔄 完整工作流程

```
用户上传图片
    ↓
服务器接收 (FastAPI)
    ↓
SHARP推理 (RTX 3090) [当前使用测试PLY]
    ↓
生成PLY文件 (63MB)
    ↓
客户端下载 (2秒)
    ↓
Bevy加载PLY
    ↓
bevy_gaussian_splatting渲染
    ↓
实时3DGS显示 (60 FPS)
```

## 🎯 下一步开发

- [ ] 解决SHARP CUDA错误
- [ ] 添加UI界面（图片选择）
- [ ] 实现进度显示
- [ ] 添加鼠标相机控制
- [ ] 批量处理功能

## 📞 技术支持

- 服务器地址: `192.168.31.164:8000`
- 项目路径: `/Users/jqwang/144-显微镜拍照-bevy-3dgs/microscope_viewer`
- 服务器路径: `/home/wjq/ml-sharp`

---

**状态**: ✅ 前后端已打通，可正常使用
**最后更新**: 2026-01-20
