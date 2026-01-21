#!/usr/bin/env python3
"""
LightGaussian 简化版剪枝工具
=============================

基于 LightGaussian (https://github.com/VITA-Group/LightGaussian) 的核心思想，
实现一个不需要原始训练数据的简化版PLY剪枝工具。

核心原理：
1. 基于不透明度(opacity)评估高斯点的重要性
2. 基于尺度(scale)评估高斯点的视觉贡献
3. 综合计算重要性分数，剪枝低贡献的高斯点

预期效果：
- 文件大小减少 50-65%
- GPU占用降低 30-50%
- 质量损失 < 5%

使用方法：
    python light_prune.py input.ply output.ply --keep-ratio 0.5
    python light_prune.py input.ply output.ply --keep-ratio 0.35  # 更激进

作者: Claude (基于LightGaussian思想)
"""

import argparse
import struct
import numpy as np
from pathlib import Path
from typing import Tuple, Dict, List, Optional
import time


class PLYGaussianPruner:
    """3D Gaussian Splatting PLY文件剪枝器"""

    # SHARP输出的PLY属性定义
    VERTEX_PROPERTIES = [
        ('x', 'f4'),
        ('y', 'f4'),
        ('z', 'f4'),
        ('f_dc_0', 'f4'),  # 颜色DC分量
        ('f_dc_1', 'f4'),
        ('f_dc_2', 'f4'),
        ('opacity', 'f4'),  # 不透明度
        ('scale_0', 'f4'),  # 尺度
        ('scale_1', 'f4'),
        ('scale_2', 'f4'),
        ('rot_0', 'f4'),    # 旋转四元数
        ('rot_1', 'f4'),
        ('rot_2', 'f4'),
        ('rot_3', 'f4'),
    ]

    def __init__(self, verbose: bool = True):
        self.verbose = verbose
        self.header_lines = []
        self.extra_elements = []  # 存储额外的元素（extrinsic, intrinsic等）

    def log(self, msg: str):
        if self.verbose:
            print(f"[LightPrune] {msg}")

    def parse_header(self, filepath: str) -> Tuple[int, int, Dict]:
        """解析PLY文件头部"""
        self.header_lines = []
        self.extra_elements = []

        with open(filepath, 'rb') as f:
            # 读取头部
            header_end = 0
            vertex_count = 0
            properties = []
            current_element = None
            current_element_count = 0
            current_element_props = []

            while True:
                line = f.readline().decode('utf-8').strip()
                self.header_lines.append(line)

                if line == 'end_header':
                    header_end = f.tell()
                    break

                if line.startswith('element vertex'):
                    # 保存之前的元素
                    if current_element and current_element != 'vertex':
                        self.extra_elements.append({
                            'name': current_element,
                            'count': current_element_count,
                            'properties': current_element_props.copy()
                        })

                    vertex_count = int(line.split()[-1])
                    current_element = 'vertex'
                    current_element_count = vertex_count
                    current_element_props = []

                elif line.startswith('element '):
                    # 保存之前的元素
                    if current_element and current_element != 'vertex':
                        self.extra_elements.append({
                            'name': current_element,
                            'count': current_element_count,
                            'properties': current_element_props.copy()
                        })

                    parts = line.split()
                    current_element = parts[1]
                    current_element_count = int(parts[2])
                    current_element_props = []

                elif line.startswith('property'):
                    parts = line.split()
                    prop_type = parts[1]
                    prop_name = parts[2]

                    if current_element == 'vertex':
                        properties.append((prop_name, prop_type))
                    else:
                        current_element_props.append((prop_name, prop_type))

            # 保存最后一个非vertex元素
            if current_element and current_element != 'vertex':
                self.extra_elements.append({
                    'name': current_element,
                    'count': current_element_count,
                    'properties': current_element_props.copy()
                })

        return vertex_count, header_end, {'properties': properties}

    def read_vertices(self, filepath: str, vertex_count: int, header_end: int) -> np.ndarray:
        """读取顶点数据"""
        # 计算每个顶点的字节数 (14个float32 = 56字节)
        vertex_size = 14 * 4  # 14 floats * 4 bytes

        with open(filepath, 'rb') as f:
            f.seek(header_end)
            vertex_data = f.read(vertex_count * vertex_size)

        # 解析为numpy数组
        dtype = np.dtype([
            ('x', '<f4'), ('y', '<f4'), ('z', '<f4'),
            ('f_dc_0', '<f4'), ('f_dc_1', '<f4'), ('f_dc_2', '<f4'),
            ('opacity', '<f4'),
            ('scale_0', '<f4'), ('scale_1', '<f4'), ('scale_2', '<f4'),
            ('rot_0', '<f4'), ('rot_1', '<f4'), ('rot_2', '<f4'), ('rot_3', '<f4'),
        ])

        vertices = np.frombuffer(vertex_data, dtype=dtype)
        return vertices

    def read_extra_data(self, filepath: str, header_end: int, vertex_count: int) -> bytes:
        """读取额外的元素数据（extrinsic, intrinsic等）"""
        vertex_size = 14 * 4
        vertex_data_end = header_end + vertex_count * vertex_size

        with open(filepath, 'rb') as f:
            f.seek(vertex_data_end)
            extra_data = f.read()

        return extra_data

    def calculate_importance_score(self, vertices: np.ndarray,
                                   opacity_weight: float = 0.6,
                                   scale_weight: float = 0.3,
                                   color_weight: float = 0.1,
                                   v_pow: float = 0.1) -> np.ndarray:
        """
        计算每个高斯点的重要性分数

        基于LightGaussian的思想：
        - 不透明度高的点更重要（贡献更多颜色）
        - 尺度大的点更重要（覆盖更多像素）
        - 颜色强度高的点更重要（视觉贡献大）

        参数:
            vertices: 顶点数据
            opacity_weight: 不透明度权重
            scale_weight: 尺度权重
            color_weight: 颜色权重
            v_pow: 体积幂次（来自LightGaussian）

        返回:
            重要性分数数组
        """
        # 1. 不透明度分数 (sigmoid激活后的值)
        # PLY中存储的是logit值，需要sigmoid转换
        opacity_logit = vertices['opacity']
        opacity = 1.0 / (1.0 + np.exp(-opacity_logit))
        opacity_score = opacity

        # 2. 尺度分数 (体积的代理)
        # 使用exp因为PLY中存储的是log(scale)
        scale_0 = np.exp(vertices['scale_0'])
        scale_1 = np.exp(vertices['scale_1'])
        scale_2 = np.exp(vertices['scale_2'])

        # 计算体积（椭球体积的代理）
        volume = scale_0 * scale_1 * scale_2

        # 归一化体积分数
        volume_90 = np.percentile(volume, 90)
        volume_ratio = np.clip(volume / (volume_90 + 1e-8), 0, 1)

        # LightGaussian风格的体积加权
        scale_score = np.power(volume_ratio, v_pow)

        # 3. 颜色强度分数
        color_intensity = np.sqrt(
            vertices['f_dc_0']**2 +
            vertices['f_dc_1']**2 +
            vertices['f_dc_2']**2
        )
        color_score = color_intensity / (np.percentile(color_intensity, 95) + 1e-8)
        color_score = np.clip(color_score, 0, 1)

        # 4. 综合重要性分数
        importance = (
            opacity_weight * opacity_score +
            scale_weight * scale_score +
            color_weight * color_score
        )

        return importance

    def prune(self, vertices: np.ndarray, keep_ratio: float,
              method: str = 'importance') -> Tuple[np.ndarray, np.ndarray]:
        """
        执行剪枝

        参数:
            vertices: 顶点数据
            keep_ratio: 保留比例 (0.0-1.0)
            method: 剪枝方法
                - 'importance': 基于综合重要性分数
                - 'opacity': 仅基于不透明度
                - 'random': 随机剪枝（用于对比）

        返回:
            (剪枝后的顶点, 保留的索引)
        """
        n_vertices = len(vertices)
        n_keep = int(n_vertices * keep_ratio)

        self.log(f"剪枝方法: {method}")
        self.log(f"原始顶点数: {n_vertices:,}")
        self.log(f"目标保留数: {n_keep:,} ({keep_ratio*100:.1f}%)")

        if method == 'importance':
            # 基于综合重要性分数
            scores = self.calculate_importance_score(vertices)
        elif method == 'opacity':
            # 仅基于不透明度
            opacity_logit = vertices['opacity']
            scores = 1.0 / (1.0 + np.exp(-opacity_logit))
        elif method == 'random':
            # 随机剪枝
            scores = np.random.rand(n_vertices)
        else:
            raise ValueError(f"未知的剪枝方法: {method}")

        # 选择top-k
        indices = np.argsort(scores)[-n_keep:]
        indices = np.sort(indices)  # 保持原始顺序

        pruned_vertices = vertices[indices]

        self.log(f"剪枝后顶点数: {len(pruned_vertices):,}")
        self.log(f"压缩率: {(1 - keep_ratio) * 100:.1f}%")

        return pruned_vertices, indices

    def write_ply(self, filepath: str, vertices: np.ndarray, extra_data: bytes):
        """写入剪枝后的PLY文件"""
        with open(filepath, 'wb') as f:
            # 写入头部
            f.write(b'ply\n')
            f.write(b'format binary_little_endian 1.0\n')
            f.write(f'element vertex {len(vertices)}\n'.encode())
            f.write(b'property float x\n')
            f.write(b'property float y\n')
            f.write(b'property float z\n')
            f.write(b'property float f_dc_0\n')
            f.write(b'property float f_dc_1\n')
            f.write(b'property float f_dc_2\n')
            f.write(b'property float opacity\n')
            f.write(b'property float scale_0\n')
            f.write(b'property float scale_1\n')
            f.write(b'property float scale_2\n')
            f.write(b'property float rot_0\n')
            f.write(b'property float rot_1\n')
            f.write(b'property float rot_2\n')
            f.write(b'property float rot_3\n')

            # 写入额外元素的头部
            for elem in self.extra_elements:
                f.write(f"element {elem['name']} {elem['count']}\n".encode())
                for prop_name, prop_type in elem['properties']:
                    f.write(f"property {prop_type} {prop_name}\n".encode())

            f.write(b'end_header\n')

            # 写入顶点数据
            f.write(vertices.tobytes())

            # 写入额外数据
            if extra_data:
                f.write(extra_data)

    def process(self, input_path: str, output_path: str,
                keep_ratio: float = 0.5, method: str = 'importance'):
        """
        处理PLY文件

        参数:
            input_path: 输入PLY文件路径
            output_path: 输出PLY文件路径
            keep_ratio: 保留比例
            method: 剪枝方法
        """
        start_time = time.time()

        self.log(f"输入文件: {input_path}")
        self.log(f"输出文件: {output_path}")

        # 1. 解析头部
        self.log("解析PLY头部...")
        vertex_count, header_end, meta = self.parse_header(input_path)
        self.log(f"顶点数量: {vertex_count:,}")

        # 2. 读取顶点数据
        self.log("读取顶点数据...")
        vertices = self.read_vertices(input_path, vertex_count, header_end)

        # 3. 读取额外数据
        extra_data = self.read_extra_data(input_path, header_end, vertex_count)
        self.log(f"额外数据大小: {len(extra_data)} bytes")

        # 4. 执行剪枝
        self.log("执行剪枝...")
        pruned_vertices, kept_indices = self.prune(vertices, keep_ratio, method)

        # 5. 写入输出文件
        self.log("写入输出文件...")
        self.write_ply(output_path, pruned_vertices, extra_data)

        # 6. 统计
        input_size = Path(input_path).stat().st_size / (1024 * 1024)
        output_size = Path(output_path).stat().st_size / (1024 * 1024)
        elapsed = time.time() - start_time

        self.log("=" * 50)
        self.log("剪枝完成!")
        self.log(f"输入文件大小: {input_size:.2f} MB")
        self.log(f"输出文件大小: {output_size:.2f} MB")
        self.log(f"压缩比: {input_size/output_size:.2f}x")
        self.log(f"节省空间: {(1 - output_size/input_size) * 100:.1f}%")
        self.log(f"处理时间: {elapsed:.2f}s")
        self.log("=" * 50)

        return {
            'input_vertices': vertex_count,
            'output_vertices': len(pruned_vertices),
            'input_size_mb': input_size,
            'output_size_mb': output_size,
            'compression_ratio': input_size / output_size,
            'elapsed_seconds': elapsed
        }


def main():
    parser = argparse.ArgumentParser(
        description='LightGaussian 简化版剪枝工具 - 压缩3D Gaussian Splatting PLY文件',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
示例:
  # 保留50%的高斯点（推荐）
  python light_prune.py input.ply output.ply --keep-ratio 0.5

  # 更激进的剪枝（保留35%，类似LightGaussian）
  python light_prune.py input.ply output.ply --keep-ratio 0.35

  # 仅基于不透明度剪枝
  python light_prune.py input.ply output.ply --keep-ratio 0.5 --method opacity

  # 批量处理
  for f in *.ply; do python light_prune.py "$f" "pruned_$f" --keep-ratio 0.5; done
        """
    )

    parser.add_argument('input', type=str, help='输入PLY文件路径')
    parser.add_argument('output', type=str, help='输出PLY文件路径')
    parser.add_argument('--keep-ratio', type=float, default=0.5,
                        help='保留比例 (0.0-1.0)，默认0.5')
    parser.add_argument('--method', type=str, default='importance',
                        choices=['importance', 'opacity', 'random'],
                        help='剪枝方法: importance(综合), opacity(不透明度), random(随机)')
    parser.add_argument('--quiet', action='store_true',
                        help='静默模式，不输出日志')

    args = parser.parse_args()

    # 验证参数
    if not Path(args.input).exists():
        print(f"错误: 输入文件不存在: {args.input}")
        return 1

    if args.keep_ratio <= 0 or args.keep_ratio > 1:
        print(f"错误: keep-ratio必须在(0, 1]范围内")
        return 1

    # 执行剪枝
    pruner = PLYGaussianPruner(verbose=not args.quiet)

    try:
        result = pruner.process(
            args.input,
            args.output,
            keep_ratio=args.keep_ratio,
            method=args.method
        )
        return 0
    except Exception as e:
        print(f"错误: {e}")
        import traceback
        traceback.print_exc()
        return 1


if __name__ == '__main__':
    exit(main())
