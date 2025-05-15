use base64::{Engine as _, engine::general_purpose};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    error::Error,
    fs,
    io::{self},
    path::{Path, PathBuf},
};
use url::Url;

// --- Default names for output subdirectories ---
pub const IMAGE_DIR_NAME: &str = "images";
pub const BASE64_DIR_NAME: &str = "base64";

// --- Custom Error Type ---
#[derive(Debug)]
pub struct AppError(pub String);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Error for AppError {}
impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError(s.to_string())
    }
}
impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError(s)
    }
}
impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        AppError(format!("IO Error: {}", err))
    }
}
impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError(format!("JSON Error: {}", err))
    }
}
impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError(format!("HTTP Request Error: {}", err))
    }
}
impl From<url::ParseError> for AppError {
    fn from(err: url::ParseError) -> Self {
        AppError(format!("URL Parse Error: {}", err))
    }
}

// --- Processing Result Enum for each file ---
#[derive(Debug)]
pub enum FileProcessResult {
    Success,
    Skipped(String),
    Failed(String, String), // file_name_for_log, error_message
}

/// Processes a single JSON file.
pub fn process_json_file(
    json_path: &PathBuf,
    image_dir_path: &Path,
    base64_dir_path: &Path,
    http_client: &Client,
    show_progress: bool,
) -> FileProcessResult {
    let file_name_os_str = match json_path.file_name() {
        Some(name) => name,
        None => {
            return FileProcessResult::Failed(
                "UnknownFile".to_string(),
                format!("Could not get file name from path: {:?}", json_path),
            );
        }
    };
    let log_file_name = file_name_os_str.to_string_lossy().to_string(); // For logging, even if not perfect UTF-8

    if !show_progress {
        println!("Processing file: {}", log_file_name);
    }

    let content = match fs::read_to_string(json_path) {
        Ok(c) => c,
        Err(e) => {
            return FileProcessResult::Failed(
                log_file_name,
                format!("Failed to read file content: {}", e),
            );
        }
    };
    let mut json_data: Value = match serde_json::from_str(&content) {
        Ok(jd) => jd,
        Err(e) => {
            return FileProcessResult::Failed(log_file_name, format!("Failed to parse JSON: {}", e));
        }
    };

    let screenshot_url_opt = json_data
        .get("screenshot")
        .and_then(Value::as_str)
        .map(String::from);

    let screenshot_url = match screenshot_url_opt {
        Some(url) if !url.is_empty() && url != "null" => url,
        _ => {
            let skip_msg = format!("No valid screenshot URL found in {}", log_file_name);
            if !show_progress {
                println!("  [SKIP] {}", skip_msg);
            }
            return FileProcessResult::Skipped(skip_msg);
        }
    };
    if !show_progress {
        println!("  Screenshot URL: {}", screenshot_url);
    }

    if !show_progress {
        println!("  Downloading image from {} ...", screenshot_url);
    }
    let response = match http_client.get(&screenshot_url).send() {
        Ok(r) => r,
        Err(e) => {
            return FileProcessResult::Failed(
                log_file_name,
                format!("HTTP request failed for {}: {}", screenshot_url, e),
            );
        }
    };

    if let Err(e) = response.error_for_status_ref() {
        return FileProcessResult::Failed(
            log_file_name,
            format!("HTTP error downloading {}: {}", screenshot_url, e),
        );
    }

    let image_bytes = match response.bytes() {
        Ok(b) => b.to_vec(),
        Err(e) => {
            return FileProcessResult::Failed(
                log_file_name,
                format!("Failed to get image bytes from {}: {}", screenshot_url, e),
            );
        }
    };

    if image_bytes.is_empty() {
        return FileProcessResult::Failed(
            log_file_name,
            format!("Downloaded image from {} is empty", screenshot_url),
        );
    }
    if !show_progress {
        println!("  Download successful ({} bytes).", image_bytes.len());
    }

    // --- MODIFIED: Image filename extraction ---
    let image_filename_to_save: String = match Url::parse(&screenshot_url) {
        Ok(parsed_url) => {
            if let Some(name) = parsed_url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|s| !s.is_empty())
            {
                name.to_string()
            } else {
                let warn_msg = format!(
                    "[WARN] Could not determine filename from URL path segments: {}. Using JSON-derived name for {}.",
                    screenshot_url, log_file_name
                );
                if show_progress {
                    eprintln!("{}", warn_msg);
                } else {
                    println!("  {}", warn_msg);
                }
                let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => {
                        return FileProcessResult::Failed(
                            log_file_name,
                            format!(
                                "Could not get valid file stem from {:?} as fallback",
                                json_path
                            ),
                        );
                    }
                };
                format!("{}.png", stem)
            }
        }
        Err(parse_err) => {
            let warn_msg = format!(
                "[WARN] Failed to parse screenshot URL '{}' for filename extraction: {}. Using JSON-derived name for {}.",
                screenshot_url, parse_err, log_file_name
            );
            if show_progress {
                eprintln!("{}", warn_msg);
            } else {
                println!("  {}", warn_msg);
            }
            let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => {
                    return FileProcessResult::Failed(
                        log_file_name,
                        format!(
                            "Could not get valid file stem from {:?} as fallback after URL parse error",
                            json_path
                        ),
                    );
                }
            };
            format!("{}.png", stem)
        }
    };

    let image_output_path = image_dir_path.join(&image_filename_to_save);
    if !show_progress {
        println!("  Image will be saved as: {}", image_filename_to_save);
    }

    if let Err(e) = fs::write(&image_output_path, &image_bytes) {
        return FileProcessResult::Failed(
            log_file_name,
            format!("Failed to save image to {:?}: {}", image_output_path, e),
        );
    }
    if !show_progress {
        println!("  Image saved to: {:?}", image_output_path);
    }

    let mime_type = match infer::get(&image_bytes) {
        Some(kind) => {
            if !show_progress {
                println!("  Detected MIME type: {}", kind.mime_type());
            }
            kind.mime_type().to_string()
        }
        None => {
            if !show_progress {
                println!(
                    "  [WARN] Could not infer MIME type. Defaulting to application/octet-stream."
                );
            }
            "application/octet-stream".to_string()
        }
    };

    let base64_encoded_image = general_purpose::STANDARD.encode(&image_bytes);
    let data_url = format!("data:{};base64,{}", mime_type, base64_encoded_image);

    let obj = match json_data.as_object_mut() {
        Some(o) => o,
        None => {
            return FileProcessResult::Failed(
                log_file_name,
                "JSON root is not an object".to_string(),
            );
        }
    };
    obj.insert("screenshot".to_string(), Value::String(data_url));

    let new_json_string = match serde_json::to_string_pretty(&json_data) {
        Ok(s) => s,
        Err(e) => {
            return FileProcessResult::Failed(
                log_file_name,
                format!("Failed to serialize new JSON: {}", e),
            );
        }
    };
    // Use the original OsStr for the output JSON filename to handle non-UTF8 filenames correctly
    let base64_json_output_path = base64_dir_path.join(file_name_os_str);

    if let Err(e) = fs::write(&base64_json_output_path, new_json_string) {
        return FileProcessResult::Failed(
            log_file_name,
            format!(
                "Failed to save base64 JSON to {:?}: {}",
                base64_json_output_path, e
            ),
        );
    }
    if !show_progress {
        println!("  Base64 JSON saved to: {:?}", base64_json_output_path);
    }

    FileProcessResult::Success
}
