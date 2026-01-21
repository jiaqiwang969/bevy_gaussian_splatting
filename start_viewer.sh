#!/bin/bash
# Microscope 3DGS Viewer - 快速启动脚本

echo "🚀 启动 Microscope 3DGS Viewer"
echo "================================"

# 检查服务器是否运行
echo "📡 检查服务器状态..."
if curl -s http://192.168.31.164:8000/ > /dev/null 2>&1; then
    echo "✅ 服务器正在运行"
else
    echo "⚠️  服务器未运行，请先启动服务器："
    echo "   ssh wjq@192.168.31.164"
    echo "   cd /home/wjq/ml-sharp"
    echo "   ./venv/bin/python server_simple.py"
    exit 1
fi

# 检查PLY文件
echo "📁 检查资源文件..."
mkdir -p target/release/assets
for f in test.ply bevy_logo.ply; do
    if [ ! -f "target/release/assets/$f" ]; then
        echo "⚠️  target/release/assets/$f 不存在，正在复制..."
        cp "assets/$f" "target/release/assets/$f"
        echo "✅ 已复制 $f"
    fi
done

# 启动客户端
echo "🎮 启动3DGS查看器..."
echo ""
echo "控制说明："
echo "  WASD - 移动相机"
echo "  Space - 向上"
echo "  Shift - 向下"
echo ""

./target/release/microscope_viewer
