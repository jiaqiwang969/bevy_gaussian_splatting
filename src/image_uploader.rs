use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::Deserialize;

/// 上传状态
#[derive(Debug, Clone, PartialEq)]
pub enum UploadStatus {
    Idle,
    SelectingFile,
    Uploading { progress: f32 },
    Processing { stage: String },
    Downloading { progress: f32 },
    Completed { ply_path: PathBuf, total_time: f32 },
    Error { message: String },
}

/// 上传状态资源
#[derive(Resource, Clone)]
pub struct ImageUploadState {
    pub status: Arc<Mutex<UploadStatus>>,
    pub server_url: String,
}

impl Default for ImageUploadState {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(UploadStatus::Idle)),
            server_url: "http://192.168.31.164:8000".to_string(),
        }
    }
}

impl ImageUploadState {
    pub fn get_status(&self) -> UploadStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&self, status: UploadStatus) {
        *self.status.lock().unwrap() = status;
    }
}

/// 触发文件选择对话框
pub fn trigger_file_dialog(upload_state: ImageUploadState) {
    std::thread::spawn(move || {
        upload_state.set_status(UploadStatus::SelectingFile);

        // 打开文件选择对话框
        let file = rfd::FileDialog::new()
            .add_filter("图片", &["jpg", "jpeg", "png", "bmp"])
            .set_title("选择要生成3DGS的图片")
            .pick_file();

        if let Some(path) = file {
            info!("📁 选择了文件: {:?}", path);
            upload_and_process(upload_state, path);
        } else {
            info!("❌ 取消选择文件");
            upload_state.set_status(UploadStatus::Idle);
        }
    });
}

/// 下载信息结构
#[derive(Debug, Deserialize)]
struct DownloadInfo {
    file_size: usize,
    chunk_size: usize,
    num_chunks: usize,
    filename: String,
}

/// 并行下载PLY文件
fn download_ply_parallel(server_url: &str, job_id: &str) -> Result<Vec<u8>, String> {
    // 1. 获取下载信息
    let info_url = format!("{}/api/download_info/{}", server_url, job_id);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建HTTP客户端失败: {}", e))?;

    let info: DownloadInfo = client
        .get(&info_url)
        .send()
        .map_err(|e| format!("获取下载信息失败: {}", e))?
        .json()
        .map_err(|e| format!("解析下载信息失败: {}", e))?;

    info!("📊 文件信息: {} bytes, {} 个块", info.file_size, info.num_chunks);

    // 2. 并行下载所有块
    let chunks: Arc<Mutex<Vec<Option<Vec<u8>>>>> = Arc::new(Mutex::new(vec![None; info.num_chunks]));
    let mut handles = vec![];

    for chunk_id in 0..info.num_chunks {
        let server_url = server_url.to_string();
        let job_id = job_id.to_string();
        let chunks = Arc::clone(&chunks);

        let handle = std::thread::spawn(move || {
            let chunk_url = format!("{}/api/download_chunk/{}/{}", server_url, job_id, chunk_id);
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match client.get(&chunk_url).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.bytes() {
                            Ok(data) => {
                                let mut chunks = chunks.lock().unwrap();
                                chunks[chunk_id] = Some(data.to_vec());
                                info!("✅ 块 {} 下载完成 ({} bytes)", chunk_id, data.len());
                            }
                            Err(e) => error!("❌ 块 {} 读取失败: {}", chunk_id, e),
                        }
                    } else {
                        error!("❌ 块 {} 下载失败: {}", chunk_id, response.status());
                    }
                }
                Err(e) => error!("❌ 块 {} 请求失败: {}", chunk_id, e),
            }
        });

        handles.push(handle);
    }

    // 3. 等待所有线程完成
    for handle in handles {
        let _ = handle.join();
    }

    // 4. 重组数据
    let chunks = chunks.lock().unwrap();
    let mut result = Vec::with_capacity(info.file_size);

    for (i, chunk) in chunks.iter().enumerate() {
        match chunk {
            Some(data) => result.extend_from_slice(data),
            None => return Err(format!("块 {} 下载失败", i)),
        }
    }

    if result.len() != info.file_size {
        return Err(format!(
            "文件大小不匹配: 预期 {} bytes, 实际 {} bytes",
            info.file_size,
            result.len()
        ));
    }

    Ok(result)
}

/// 上传图片并处理
fn upload_and_process(upload_state: ImageUploadState, image_path: PathBuf) {
    let start_time = Instant::now();

    // 读取图片文件
    upload_state.set_status(UploadStatus::Uploading { progress: 0.0 });

    let image_data = match std::fs::read(&image_path) {
        Ok(data) => data,
        Err(e) => {
            error!("❌ 读取图片失败: {}", e);
            upload_state.set_status(UploadStatus::Error {
                message: format!("读取图片失败: {}", e),
            });
            return;
        }
    };

    info!("📤 开始上传图片 ({:.2} MB)...", image_data.len() as f32 / 1_000_000.0);
    upload_state.set_status(UploadStatus::Uploading { progress: 0.5 });

    // 构建multipart表单
    let file_name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.jpg")
        .to_string();

    let form = reqwest::blocking::multipart::Form::new()
        .part(
            "image",
            reqwest::blocking::multipart::Part::bytes(image_data)
                .file_name(file_name.clone())
                .mime_str("image/jpeg")
                .unwrap(),
        );

    upload_state.set_status(UploadStatus::Uploading { progress: 1.0 });

    // 发送请求
    let url = format!("{}/api/predict", upload_state.server_url);
    info!("🚀 发送请求到: {}", url);

    upload_state.set_status(UploadStatus::Processing {
        stage: "SHARP推理中 (预计0.5秒)...".to_string(),
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap();

    let response = match client.post(&url).multipart(form).send() {
        Ok(resp) => resp,
        Err(e) => {
            error!("❌ 请求失败: {}", e);
            upload_state.set_status(UploadStatus::Error {
                message: format!("请求失败: {}", e),
            });
            return;
        }
    };

    if !response.status().is_success() {
        error!("❌ 服务器返回错误: {}", response.status());
        upload_state.set_status(UploadStatus::Error {
            message: format!("服务器错误: {}", response.status()),
        });
        return;
    }

    // 获取job_id
    #[derive(Deserialize)]
    struct PredictResponse {
        job_id: String,
    }

    let job_response: PredictResponse = match response.json() {
        Ok(data) => data,
        Err(e) => {
            error!("❌ 解析响应失败: {}", e);
            upload_state.set_status(UploadStatus::Error {
                message: format!("解析响应失败: {}", e),
            });
            return;
        }
    };

    info!("✅ SHARP推理完成，开始并行下载PLY...");
    upload_state.set_status(UploadStatus::Downloading { progress: 0.0 });

    // 使用并行下载
    let ply_data = match download_ply_parallel(&upload_state.server_url, &job_response.job_id) {
        Ok(data) => data,
        Err(e) => {
            error!("❌ 并行下载失败: {}", e);
            upload_state.set_status(UploadStatus::Error {
                message: format!("下载失败: {}", e),
            });
            return;
        }
    };

    info!("📥 并行下载完成 ({:.2} MB)", ply_data.len() as f32 / 1_000_000.0);
    upload_state.set_status(UploadStatus::Downloading { progress: 1.0 });

    // 保存PLY文件到assets目录
    let output_path = PathBuf::from("assets/generated.ply");
    if let Err(e) = std::fs::write(&output_path, &ply_data) {
        error!("❌ 保存PLY失败: {}", e);
        upload_state.set_status(UploadStatus::Error {
            message: format!("保存PLY失败: {}", e),
        });
        return;
    }

    let total_time = start_time.elapsed().as_secs_f32();
    info!("🎉 完成！总耗时: {:.2}秒", total_time);
    info!("📁 PLY文件已保存到: {:?}", output_path);

    upload_state.set_status(UploadStatus::Completed {
        ply_path: output_path,
        total_time,
    });
}
