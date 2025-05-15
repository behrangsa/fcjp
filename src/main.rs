use clap::Parser;
use indicatif::{ParallelProgressIterator, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

// Import functionality from our library
use fcjp::{AppError, BASE64_DIR_NAME, FileProcessResult, IMAGE_DIR_NAME, process_json_file};

// --- Command-Line Arguments Definition ---
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
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

fn main() -> Result<(), Box<dyn Error>> {
    let cli_args = CliArgs::parse();

    println!(
        "Starting screenshot processor (Rust v{})...",
        env!("CARGO_PKG_VERSION")
    );

    if !cli_args.directory.exists() {
        return Err(Box::new(AppError(format!(
            "Input directory does not exist: {:?}",
            cli_args.directory
        ))));
    }
    if !cli_args.directory.is_dir() {
        return Err(Box::new(AppError(format!(
            "Input path is not a directory: {:?}",
            cli_args.directory
        ))));
    }
    let canonical_input_path = fs::canonicalize(&cli_args.directory)?;
    println!("Input directory for JSON files: {:?}", canonical_input_path);

    let image_dir_path = cli_args
        .image_output_directory
        .unwrap_or_else(|| canonical_input_path.join(IMAGE_DIR_NAME));
    let base64_dir_path = cli_args
        .base64_output_directory
        .unwrap_or_else(|| canonical_input_path.join(BASE64_DIR_NAME));

    fs::create_dir_all(&image_dir_path)?;
    println!(
        "Image output directory: {:?}",
        fs::canonicalize(&image_dir_path)?
    );
    fs::create_dir_all(&base64_dir_path)?;
    println!(
        "Base64 JSON output directory: {:?}",
        fs::canonicalize(&base64_dir_path)?
    );

    rayon::ThreadPoolBuilder::new()
        .num_threads(cli_args.concurrency)
        .build_global()?;
    println!("Using {} concurrent jobs.", cli_args.concurrency);
    println!();

    let json_files_to_process: Vec<PathBuf> = fs::read_dir(&canonical_input_path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
        .collect();

    if json_files_to_process.is_empty() {
        println!(
            "No .json files found in the input directory: {:?}",
            canonical_input_path
        );
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

    let http_client = Arc::new(
        reqwest::blocking::Client::builder()
            .user_agent(format!("ScreenshotProcessor/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(60))
            .build()?,
    );

    let processed_successfully = AtomicUsize::new(0);
    let skipped_files = AtomicUsize::new(0);
    let failed_to_process = AtomicUsize::new(0);

    let show_ind_progress = cli_args.progress; // Capture this for the closure

    json_files_to_process
        .par_iter()
        .progress_with(pb_option.clone().unwrap_or_else(ProgressBar::hidden))
        .for_each(|json_path| {
            let client_clone = Arc::clone(&http_client);
            let result = process_json_file(
                json_path,
                &image_dir_path,
                &base64_dir_path,
                &client_clone,
                show_ind_progress,
            );
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
    println!(
        "Processed successfully:    {}",
        processed_successfully.load(Ordering::SeqCst)
    );
    println!(
        "Skipped (e.g., no URL):  {}",
        skipped_files.load(Ordering::SeqCst)
    );
    println!(
        "Failed to process:       {}",
        failed_to_process.load(Ordering::SeqCst)
    );
    println!("----------------------------------------");

    if failed_to_process.load(Ordering::SeqCst) > 0 {
        return Err(Box::new(AppError(format!(
            "{} files failed to process.",
            failed_to_process.load(Ordering::SeqCst)
        ))));
    }

    Ok(())
}
