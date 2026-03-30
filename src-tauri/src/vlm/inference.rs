use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{DynamicImage, GenericImageView, ImageFormat};
use reqwest::Client;
use serde_json::json;
use tokio::{task::spawn_blocking, time::sleep};

use crate::{
    models::{default_system_prompt, default_user_prompt},
    vlm::server::VlmError,
};

const MAX_IMAGE_WIDTH: u32 = 1280;
const MAX_IMAGE_HEIGHT: u32 = 720;
const REQUEST_TIMEOUT_SECS: u64 = 60;
const MAX_RETRIES: usize = 3;
const INITIAL_BACKOFF_MS: u64 = 500;

pub struct PromptContext<'a> {
    pub app: Option<&'a str>,
    pub window_title: Option<&'a str>,
    pub system_prompt: Option<&'a str>,
    pub user_prompt: Option<&'a str>,
}

pub async fn describe_screenshot(
    client: &Client,
    image_path: &Path,
    server_url: &str,
    max_tokens: u32,
    prompt_context: PromptContext<'_>,
) -> Result<String, VlmError> {
    let started_at = Instant::now();
    let image_b64 = load_resized_image_base64(image_path).await?;
    let endpoint = format!("{}/v1/chat/completions", server_url.trim_end_matches('/'));
    let system_prompt = resolve_prompt(prompt_context.system_prompt, default_system_prompt);
    let user_prompt = build_user_prompt(&prompt_context);

    for attempt in 0..MAX_RETRIES {
        match request_description(
            client,
            &endpoint,
            &image_b64,
            max_tokens,
            &system_prompt,
            &user_prompt,
        )
        .await
        {
            Ok(description) => {
                eprintln!("vlm inference completed in {:?}", started_at.elapsed());
                return Ok(description);
            }
            Err(error) if attempt + 1 < MAX_RETRIES && should_retry(&error) => {
                let backoff = INITIAL_BACKOFF_MS * (1_u64 << attempt);
                sleep(Duration::from_millis(backoff)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(VlmError::InvalidResponse(
        "VLM inference exhausted all retries".to_string(),
    ))
}

async fn request_description(
    client: &Client,
    endpoint: &str,
    image_b64: &str,
    max_tokens: u32,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, VlmError> {
    let payload = json!({
        "model": "qwen",
        "messages": [{
            "role": "system",
            "content": system_prompt
        }, {
            "role": "user",
            "content": [{
                "type": "image_url",
                "image_url": {
                    "url": format!("data:image/png;base64,{image_b64}")
                }
            }, {
                "type": "text",
                "text": user_prompt
            }]
        }],
        "max_tokens": max_tokens,
        "temperature": 0.1
    });

    let response = client
        .post(endpoint)
        .json(&payload)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(VlmError::UnexpectedStatus(response.status().as_u16()));
    }

    let response = response.json::<serde_json::Value>().await?;
    extract_description(&response)
}

async fn load_resized_image_base64(image_path: &Path) -> Result<String, VlmError> {
    let image_path = image_path.to_path_buf();
    let bytes = spawn_blocking(move || encode_resized_image(&image_path)).await??;
    Ok(BASE64.encode(bytes))
}

fn encode_resized_image(image_path: &PathBuf) -> Result<Vec<u8>, VlmError> {
    let image = image::open(image_path)?;
    let resized = resize_for_vlm(image);
    let mut output = Cursor::new(Vec::new());
    resized.write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn resize_for_vlm(image: DynamicImage) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= MAX_IMAGE_WIDTH && height <= MAX_IMAGE_HEIGHT {
        return image;
    }

    image.resize(
        MAX_IMAGE_WIDTH,
        MAX_IMAGE_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    )
}

fn extract_description(response: &serde_json::Value) -> Result<String, VlmError> {
    let text = response["choices"][0]["message"]["content"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            VlmError::InvalidResponse("VLM response did not contain message content".to_string())
        })?;

    Ok(text.to_string())
}

fn resolve_prompt(prompt: Option<&str>, fallback: fn() -> String) -> String {
    prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(fallback)
}

fn build_user_prompt(prompt_context: &PromptContext<'_>) -> String {
    let base_prompt = resolve_prompt(prompt_context.user_prompt, default_user_prompt);
    let mut context_lines = Vec::new();

    if let Some(app) = prompt_context
        .app
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        context_lines.push(format!("- アクティブアプリ: {app}"));
    }

    if let Some(window_title) = prompt_context
        .window_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        context_lines.push(format!("- ウィンドウタイトル: {window_title}"));
    }

    if context_lines.is_empty() {
        return base_prompt;
    }

    format!(
        "{base_prompt}\n\n補助コンテキスト:\n{}\n\n補助コンテキストは参考情報として扱い、画面から確認できる内容を優先してください。",
        context_lines.join("\n")
    )
}

fn should_retry(error: &VlmError) -> bool {
    matches!(
        error,
        VlmError::Http(_) | VlmError::UnexpectedStatus(_) | VlmError::InvalidResponse(_)
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        net::SocketAddr,
        path::{Path, PathBuf},
        process,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use base64::Engine as _;
    use image::{ImageBuffer, Rgba};
    use reqwest::Client;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{
        describe_screenshot, resize_for_vlm, PromptContext, MAX_IMAGE_HEIGHT, MAX_IMAGE_WIDTH,
    };

    fn test_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!(
            "kiroku-vlm-inference-{test_name}-{}-{unique}",
            process::id()
        ))
    }

    fn write_sample_image(path: &Path, width: u32, height: u32) {
        let image = ImageBuffer::from_pixel(width, height, Rgba([10_u8, 20_u8, 30_u8, 255_u8]));
        image
            .save(path)
            .expect("sample image should be written successfully");
    }

    async fn spawn_mock_server(
        statuses: Vec<&'static str>,
        captured_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
    ) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("address should resolve");
        let counter = Arc::new(AtomicUsize::new(0));
        let statuses = Arc::new(statuses);

        tokio::spawn(async move {
            loop {
                let accepted = listener.accept().await;
                let Ok((mut stream, _)) = accepted else {
                    break;
                };

                let counter = counter.clone();
                let statuses = statuses.clone();
                let captured_bodies = captured_bodies.clone();

                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 128 * 1024];
                    let read = stream.read(&mut buffer).await.expect("request should read");
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let body = request
                        .split("\r\n\r\n")
                        .nth(1)
                        .expect("request body should exist");
                    let body_json: serde_json::Value =
                        serde_json::from_str(body).expect("request body should be JSON");
                    captured_bodies
                        .lock()
                        .expect("captured bodies should lock")
                        .push(body_json);

                    let attempt = counter.fetch_add(1, Ordering::SeqCst);
                    let status = statuses.get(attempt).copied().unwrap_or("HTTP/1.1 200 OK");
                    let response_body = if status.ends_with("200 OK") {
                        r#"{"choices":[{"message":{"content":"Excel で売上表を更新している。"}}]}"#
                    } else {
                        r#"{"error":"temporary"}"#
                    };
                    let response = format!(
                        "{status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });

        addr
    }

    #[test]
    fn resize_for_vlm_keeps_image_within_bounds() {
        let image = image::DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            2560,
            1440,
            Rgba([0_u8, 0_u8, 0_u8, 255_u8]),
        ));

        let resized = resize_for_vlm(image);
        assert!(resized.width() <= MAX_IMAGE_WIDTH);
        assert!(resized.height() <= MAX_IMAGE_HEIGHT);
    }

    #[tokio::test]
    async fn describe_screenshot_resizes_image_and_posts_openai_payload() {
        let dir = test_dir("payload");
        fs::create_dir_all(&dir).expect("test directory should exist");
        let image_path = dir.join("sample.png");
        write_sample_image(&image_path, 2560, 1440);

        let bodies = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_mock_server(vec!["HTTP/1.1 200 OK"], bodies.clone()).await;
        let client = Client::new();

        let description = describe_screenshot(
            &client,
            &image_path,
            &format!("http://{}:{}", addr.ip(), addr.port()),
            256,
            PromptContext {
                app: Some("excel.exe"),
                window_title: Some("月次決算.xlsx"),
                system_prompt: None,
                user_prompt: None,
            },
        )
        .await
        .expect("description should be generated");

        assert_eq!(description, "Excel で売上表を更新している。");

        let request = bodies
            .lock()
            .expect("captured bodies should lock")
            .pop()
            .expect("request body should exist");
        let data_url = request["messages"][1]["content"][0]["image_url"]["url"]
            .as_str()
            .expect("image data url should exist");
        let image_b64 = data_url
            .strip_prefix("data:image/png;base64,")
            .expect("data url prefix should exist");
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(image_b64)
            .expect("base64 image should decode");
        let image = image::load_from_memory(&image_bytes).expect("payload image should decode");

        assert!(image.width() <= MAX_IMAGE_WIDTH);
        assert!(image.height() <= MAX_IMAGE_HEIGHT);

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[tokio::test]
    async fn describe_screenshot_retries_after_transient_failure() {
        let dir = test_dir("retry");
        fs::create_dir_all(&dir).expect("test directory should exist");
        let image_path = dir.join("sample.png");
        write_sample_image(&image_path, 640, 360);

        let bodies = Arc::new(Mutex::new(Vec::new()));
        let addr = spawn_mock_server(
            vec!["HTTP/1.1 500 Internal Server Error", "HTTP/1.1 200 OK"],
            bodies,
        )
        .await;
        let client = Client::new();

        let description = describe_screenshot(
            &client,
            &image_path,
            &format!("http://{}:{}", addr.ip(), addr.port()),
            256,
            PromptContext {
                app: None,
                window_title: None,
                system_prompt: None,
                user_prompt: None,
            },
        )
        .await
        .expect("description should succeed after retry");

        assert_eq!(description, "Excel で売上表を更新している。");

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }
}
