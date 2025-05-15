use std::{
    fs,
    io::{self},
    path::{Path, PathBuf},
    error::Error,
};
use serde_json::Value;
use base64::{Engine as _, engine::{general_purpose}};
use reqwest;
use infer;
use clap::Parser; // Import clap

// --- Configuration for output directories (relative to current working dir) ---
// INPUT_DIR is now handled by clap
const IMAGE_DIR: &str = "./images";
const BASE64_DIR: &str = "./base64";

// --- Command-Line Arguments Definition ---
#[derive(Parser, Debug)]
#[command(name = "screenshot-processor")]
#[command(author = "AI Assistant <no-reply@example.com>")]
#[command(version = "0.1.1")]
#[command(about = "Downloads screenshots from JSON files and embeds them as base64 data URLs.", long_about = None)]
struct CliArgs {
    /// Directory containing the JSON files to process.
    #[arg(short, long, value_name = "SOURCE_DIRECTORY", default_value = ".")]
    directory: PathBuf,

    /// Directory to save downloaded images.
    #[arg(long = "image-out", value_name = "IMAGE_OUTPUT_DIR", default_value = IMAGE_DIR)]
    image_output_directory: PathBuf,

    /// Directory to save JSON files with base64 encoded images.
    #[arg(long = "base64-out", value_name = "BASE64_OUTPUT_DIR", default_value = BASE64_DIR)]
    base64_output_directory: PathBuf,
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

/// Processes a single JSON file.
/// Returns Ok(true) if processed successfully and files were written.
/// Returns Ok(false) if skipped (e.g., no URL, or URL is empty/null string).
/// Returns Err(AppError) if an error occurred during processing.
fn process_json_file(json_path: &PathBuf, image_dir_path: &Path, base64_dir_path: &Path) -> Result<bool, AppError> {
    let file_name_os_str = json_path.file_name().ok_or_else(|| AppError(format!("Could not get file name from path: {:?}", json_path)))?;
    let file_name_str = file_name_os_str.to_str().ok_or_else(|| AppError(format!("File name is not valid UTF-8: {:?}", file_name_os_str)))?;

    println!("Processing file: {}", file_name_str);

    let content = fs::read_to_string(json_path)?;
    let mut json_data: Value = serde_json::from_str(&content)?;

    let screenshot_url_opt = json_data
        .get("screenshot")
        .and_then(Value::as_str)
        .map(String::from);

    let screenshot_url = match screenshot_url_opt {
        Some(url) if !url.is_empty() && url != "null" => url,
        _ => {
            println!("  [SKIP] No valid screenshot URL found in {}", file_name_str);
            return Ok(false);
        }
    };
    println!("  Screenshot URL: {}", screenshot_url);

    println!("  Downloading image from {} ...", screenshot_url);
    let response = reqwest::blocking::get(&screenshot_url)?;
    if !response.status().is_success() {
        return Err(AppError(format!("Failed to download image from {}: HTTP Status {}", screenshot_url, response.status())));
    }
    let image_bytes = response.bytes()?.to_vec();
    if image_bytes.is_empty() {
        return Err(AppError(format!("Downloaded image from {} is empty", screenshot_url)));
    }
    println!("  Download successful ({} bytes).", image_bytes.len());

    let stem = json_path.file_stem().ok_or_else(|| AppError(format!("Could not get file stem from {:?}", json_path)))?;
    let image_filename = PathBuf::from(stem).with_extension("png");
    let image_output_path = image_dir_path.join(image_filename);

    fs::write(&image_output_path, &image_bytes)?;
    println!("  Image saved to: {:?}", image_output_path);

    let mime_type = match infer::get(&image_bytes) {
        Some(kind) => {
            println!("  Detected MIME type: {}", kind.mime_type());
            kind.mime_type().to_string()
        }
        None => {
            println!("  [WARN] Could not infer MIME type. Defaulting to application/octet-stream.");
            "application/octet-stream".to_string()
        }
    };

    let base64_encoded_image = general_purpose::STANDARD.encode(&image_bytes);
    let data_url = format!("data:{};base64,{}", mime_type, base64_encoded_image);

    let obj = json_data.as_object_mut().ok_or_else(|| AppError("JSON root is not an object".to_string()))?;
    obj.insert("screenshot".to_string(), Value::String(data_url));

    let new_json_string = serde_json::to_string_pretty(&json_data)?;
    let base64_json_output_path = base64_dir_path.join(file_name_str);
    fs::write(&base64_json_output_path, new_json_string)?;
    println!("  Base64 JSON saved to: {:?}", base64_json_output_path);

    Ok(true)
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli_args = CliArgs::parse(); // Parse command-line arguments

    println!("Starting screenshot download and JSON base64 conversion (Rust version)...");

    // Use directories from command-line arguments or their defaults
    let input_path = &cli_args.directory;
    let image_dir_path = &cli_args.image_output_directory;
    let base64_dir_path = &cli_args.base64_output_directory;

    // Create output directories if they don't exist
    fs::create_dir_all(image_dir_path)?;
    println!("Image output directory: {:?}", fs::canonicalize(image_dir_path)?);
    fs::create_dir_all(base64_dir_path)?;
    println!("Base64 JSON output directory: {:?}", fs::canonicalize(base64_dir_path)?);

    println!("Input directory for JSON files: {:?}", fs::canonicalize(input_path)?);
    println!();

    let mut total_files_found = 0;
    let mut processed_successfully = 0;
    let mut skipped_files = 0;
    let mut failed_to_process = 0;

    let entries = fs::read_dir(input_path)?
        .collect::<Result<Vec<_>, io::Error>>()?;

    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "json" {
                    total_files_found += 1;
                    match process_json_file(&path, image_dir_path, base64_dir_path) {
                        Ok(true) => {
                            processed_successfully += 1;
                        }
                        Ok(false) => {
                            skipped_files += 1;
                        }
                        Err(e) => {
                            failed_to_process += 1;
                            let filename_for_error = path.file_name().map_or_else(|| Path::new("unknown_file").as_os_str(), |name| name);
                            eprintln!("  [ERROR] Processing {:?} failed: {}", filename_for_error, e);
                        }
                    }
                    println!();
                }
            }
        }
    }

    println!("----------------------------------------");
    println!("Processing Complete.");
    if total_files_found == 0 {
        println!("No .json files found in the input directory: {:?}", input_path);
    } else {
        println!("Total JSON files found:    {}", total_files_found);
        println!("Processed successfully:    {}", processed_successfully);
        println!("Skipped (e.g., no URL):  {}", skipped_files);
        println!("Failed to process:       {}", failed_to_process);
    }
    println!("----------------------------------------");

    if failed_to_process > 0 {
        return Err(Box::new(AppError(format!("{} files failed to process.", failed_to_process))));
    }

    Ok(())
}
