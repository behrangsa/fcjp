use std::{
    fs,
    io::{self},
    path::{Path, PathBuf},
    error::Error,
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
};
use serde_json::Value;
use base64::{Engine as _, engine::{general_purpose}};
use reqwest::blocking::Client;
use infer;
use clap::Parser;
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle, ParallelProgressIterator};
use url::Url; // Import the Url type

// --- Default names for output subdirectories ---
const IMAGE_DIR_NAME: &str = "images";
const BASE64_DIR_NAME: &str = "base64";

// --- Command-Line Arguments Definition ---
#[derive(Parser, Debug)]
#[command(name = "screenshot-processor")]
#[command(author = "AI Assistant")]
#[command(version = "0.2.1")]
#[command(about = "Downloads screenshots from JSON files and embeds them as base64 data URLs.", long_about = None)]
struct CliArgs {
    /// Directory containing the JSON files to process.
    #[arg(short, long, value_name = "SOURCE_DIRECTORY")]
    directory: PathBuf,

    /// Directory to save downloaded images.
    /// If not specified, defaults to an 'images' subdirectory within the source directory.
    #[arg(long = "image-out", value_name = "IMAGE_OUTPUT_DIR")]
    image_output_directory: Option<PathBuf>,

    /// Directory to save JSON files with base64 encoded images.
    /// If not specified, defaults to a 'base64' subdirectory within the source directory.
    #[arg(long = "base64-out", value_name = "BASE64_OUTPUT_DIR")]
    base64_output_directory: Option<PathBuf>,

    /// Number of concurrent jobs to run.
    #[arg(short, long, value_name = "NUM_JOBS", default_value_t = 4)]
    concurrency: usize,

    /// Display a progress bar.
    #[arg(long)]
    progress: bool,
}

// --- Custom Error Type ---
#[derive(Debug)]
struct AppError(String);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Error for AppError {}
impl From<&str> for AppError { fn from(s: &str) -> Self { AppError(s.to_string()) } }
impl From<String> for AppError { fn from(s: String) -> Self { AppError(s) } }
impl From<io::Error> for AppError { fn from(err: io::Error) -> Self { AppError(format!("IO Error: {}", err)) } }
impl From<serde_json::Error> for AppError { fn from(err: serde_json::Error) -> Self { AppError(format!("JSON Error: {}", err)) } }
impl From<reqwest::Error> for AppError { fn from(err: reqwest::Error) -> Self { AppError(format!("HTTP Request Error: {}", err)) } }
impl From<url::ParseError> for AppError { fn from(err: url::ParseError) -> Self { AppError(format!("URL Parse Error: {}", err)) } }


// --- Processing Result Enum for each file ---
enum FileProcessResult {
    Success,
    Skipped(String),
    Failed(String, String), // file_name_for_log, error_message
}

/// Processes a single JSON file.
fn process_json_file(
    json_path: &PathBuf,
    image_dir_path: &Path,
    base64_dir_path: &Path,
    http_client: &Client,
    show_progress: bool,
) -> FileProcessResult {
    let file_name_os_str = match json_path.file_name() {
        Some(name) => name,
        None => return FileProcessResult::Failed("UnknownFile".to_string(), format!("Could not get file name from path: {:?}", json_path)),
    };
    let log_file_name = file_name_os_str.to_string_lossy().to_string(); // For logging, even if not perfect UTF-8

    if !show_progress {
        println!("Processing file: {}", log_file_name);
    }

    let content = match fs::read_to_string(json_path) {
        Ok(c) => c,
        Err(e) => return FileProcessResult::Failed(log_file_name, format!("Failed to read file content: {}", e)),
    };
    let mut json_data: Value = match serde_json::from_str(&content) {
        Ok(jd) => jd,
        Err(e) => return FileProcessResult::Failed(log_file_name, format!("Failed to parse JSON: {}", e)),
    };

    let screenshot_url_opt = json_data
        .get("screenshot")
        .and_then(Value::as_str)
        .map(String::from);

    let screenshot_url = match screenshot_url_opt {
        Some(url) if !url.is_empty() && url != "null" => url,
        _ => {
            let skip_msg = format!("No valid screenshot URL found in {}", log_file_name);
            if !show_progress { println!("  [SKIP] {}", skip_msg); }
            return FileProcessResult::Skipped(skip_msg);
        }
    };
    if !show_progress { println!("  Screenshot URL: {}", screenshot_url); }

    if !show_progress { println!("  Downloading image from {} ...", screenshot_url); }
    let response = match http_client.get(&screenshot_url).send() {
        Ok(r) => r,
        Err(e) => return FileProcessResult::Failed(log_file_name, format!("HTTP request failed for {}: {}", screenshot_url, e)),
    };

    if let Err(e) = response.error_for_status_ref() {
        return FileProcessResult::Failed(log_file_name, format!("HTTP error downloading {}: {}", screenshot_url, e));
    }

    let image_bytes = match response.bytes() {
        Ok(b) => b.to_vec(),
        Err(e) => return FileProcessResult::Failed(log_file_name, format!("Failed to get image bytes from {}: {}", screenshot_url, e)),
    };

    if image_bytes.is_empty() {
        return FileProcessResult::Failed(log_file_name, format!("Downloaded image from {} is empty", screenshot_url));
    }
    if !show_progress { println!("  Download successful ({} bytes).", image_bytes.len()); }

    // --- MODIFIED: Image filename extraction ---
    let image_filename_to_save: String = match Url::parse(&screenshot_url) {
        Ok(parsed_url) => {
            if let Some(name) = parsed_url.path_segments().and_then(|s| s.last()).filter(|s| !s.is_empty()) {
                name.to_string()
            } else {
                let warn_msg = format!("[WARN] Could not determine filename from URL path segments: {}. Using JSON-derived name for {}.", screenshot_url, log_file_name);
                if show_progress { eprintln!("{}", warn_msg); } else { println!("  {}", warn_msg); }
                let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => return FileProcessResult::Failed(log_file_name, format!("Could not get valid file stem from {:?} as fallback", json_path)),
                };
                format!("{}.png", stem)
            }
        }
        Err(parse_err) => {
            let warn_msg = format!("[WARN] Failed to parse screenshot URL '{}' for filename extraction: {}. Using JSON-derived name for {}.", screenshot_url, parse_err, log_file_name);
            if show_progress { eprintln!("{}", warn_msg); } else { println!("  {}", warn_msg); }
            let stem = match json_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => return FileProcessResult::Failed(log_file_name, format!("Could not get valid file stem from {:?} as fallback after URL parse error", json_path)),
            };
            format!("{}.png", stem)
        }
    };

    let image_output_path = image_dir_path.join(&image_filename_to_save);
    if !show_progress { println!("  Image will be saved as: {}", image_filename_to_save); }


    if let Err(e) = fs::write(&image_output_path, &image_bytes) {
        return FileProcessResult::Failed(log_file_name, format!("Failed to save image to {:?}: {}", image_output_path, e));
    }
    if !show_progress { println!("  Image saved to: {:?}", image_output_path); }

    let mime_type = match infer::get(&image_bytes) {
        Some(kind) => {
            if !show_progress { println!("  Detected MIME type: {}", kind.mime_type()); }
            kind.mime_type().to_string()
        }
        None => {
            if !show_progress { println!("  [WARN] Could not infer MIME type. Defaulting to application/octet-stream."); }
            "application/octet-stream".to_string()
        }
    };

    let base64_encoded_image = general_purpose::STANDARD.encode(&image_bytes);
    let data_url = format!("data:{};base64,{}", mime_type, base64_encoded_image);

    let obj = match json_data.as_object_mut() {
        Some(o) => o,
        None => return FileProcessResult::Failed(log_file_name, "JSON root is not an object".to_string()),
    };
    obj.insert("screenshot".to_string(), Value::String(data_url));

    let new_json_string = match serde_json::to_string_pretty(&json_data) {
        Ok(s) => s,
        Err(e) => return FileProcessResult::Failed(log_file_name, format!("Failed to serialize new JSON: {}", e)),
    };
    // Use the original OsStr for the output JSON filename to handle non-UTF8 filenames correctly
    let base64_json_output_path = base64_dir_path.join(file_name_os_str);

    if let Err(e) = fs::write(&base64_json_output_path, new_json_string) {
        return FileProcessResult::Failed(log_file_name, format!("Failed to save base64 JSON to {:?}: {}", base64_json_output_path, e));
    }
    if !show_progress { println!("  Base64 JSON saved to: {:?}", base64_json_output_path); }

    FileProcessResult::Success
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli_args = CliArgs::parse();

    println!("Starting screenshot processor (Rust v{})...", env!("CARGO_PKG_VERSION"));

    if !cli_args.directory.exists() {
        return Err(Box::new(AppError(format!("Input directory does not exist: {:?}", cli_args.directory))));
    }
    if !cli_args.directory.is_dir() {
        return Err(Box::new(AppError(format!("Input path is not a directory: {:?}", cli_args.directory))));
    }
    let canonical_input_path = fs::canonicalize(&cli_args.directory)?;
    println!("Input directory for JSON files: {:?}", canonical_input_path);

    let image_dir_path = cli_args.image_output_directory
        .unwrap_or_else(|| canonical_input_path.join(IMAGE_DIR_NAME));
    let base64_dir_path = cli_args.base64_output_directory
        .unwrap_or_else(|| canonical_input_path.join(BASE64_DIR_NAME));

    fs::create_dir_all(&image_dir_path)?;
    println!("Image output directory: {:?}", fs::canonicalize(&image_dir_path)?);
    fs::create_dir_all(&base64_dir_path)?;
    println!("Base64 JSON output directory: {:?}", fs::canonicalize(&base64_dir_path)?);

    rayon::ThreadPoolBuilder::new()
        .num_threads(cli_args.concurrency)
        .build_global()?;
    println!("Using {} concurrent jobs.", cli_args.concurrency);
    println!();


    let json_files_to_process: Vec<PathBuf> = fs::read_dir(&canonical_input_path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().map_or(false, |ext| ext == "json"))
        .collect();

    if json_files_to_process.is_empty() {
        println!("No .json files found in the input directory: {:?}", canonical_input_path);
        return Ok(());
    }

    let total_files_found = json_files_to_process.len();
    println!("Found {} JSON file(s) to process.", total_files_found);

    let pb_option = if cli_args.progress {
        let bar = ProgressBar::new(total_files_found as u64);
        bar.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")?
            .progress_chars("=>-")); // Changed progress chars for variety
        Some(bar)
    } else {
        None
    };

    let http_client = Arc::new(Client::builder()
        .user_agent(format!("ScreenshotProcessor/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()?);

    let processed_successfully = AtomicUsize::new(0);
    let skipped_files = AtomicUsize::new(0);
    let failed_to_process = AtomicUsize::new(0);

    let show_ind_progress = cli_args.progress; // Capture this for the closure

    json_files_to_process
        .par_iter()
        .progress_with(pb_option.clone().unwrap_or_else(ProgressBar::hidden))
        .for_each(|json_path| {
            let client_clone = Arc::clone(&http_client);
            let result = process_json_file(json_path, &image_dir_path, &base64_dir_path, &client_clone, show_ind_progress);
            match result {
                FileProcessResult::Success => {
                    processed_successfully.fetch_add(1, Ordering::SeqCst);
                }
                FileProcessResult::Skipped(reason) => {
                    skipped_files.fetch_add(1, Ordering::SeqCst);
                    if !show_ind_progress {
                        eprintln!("[SKIP] {}", reason);
                    } else if let Some(pb) = &pb_option {
                        pb.println(format!("[SKIP] {}", reason)); // Print skip message above progress bar
                    }
                }
                FileProcessResult::Failed(file_name, error_msg) => {
                    failed_to_process.fetch_add(1, Ordering::SeqCst);
                    if let Some(pb) = &pb_option {
                        pb.println(format!("[ERROR] File '{}': {}", file_name, error_msg)); // Print error above progress bar
                    } else {
                        eprintln!("[ERROR] File '{}': {}", file_name, error_msg);
                    }
                }
            }
        });

    if let Some(bar) = pb_option {
        bar.finish_with_message("All files processed.");
    }

    println!("----------------------------------------");
    println!("Processing Summary:");
    println!("Total JSON files found:    {}", total_files_found);
    println!("Processed successfully:    {}", processed_successfully.load(Ordering::SeqCst));
    println!("Skipped (e.g., no URL):  {}", skipped_files.load(Ordering::SeqCst));
    println!("Failed to process:       {}", failed_to_process.load(Ordering::SeqCst));
    println!("----------------------------------------");

    if failed_to_process.load(Ordering::SeqCst) > 0 {
        return Err(Box::new(AppError(format!("{} files failed to process.", failed_to_process.load(Ordering::SeqCst)))));
    }

    Ok(())
}
