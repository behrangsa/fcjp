use base64::{Engine as _, engine::general_purpose};
use mockito::Server as MockServer; // Using mockito for simpler HTTP mocking
use serde_json::{Value, json};
use std::error::Error;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

// Import functionalities directly from the library
use fcjp::{FileProcessResult, process_json_file};

// Helper function to create test JSON files
fn create_test_json_file(
    dir: &Path,
    filename: &str,
    screenshot_url: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let json_path = dir.join(filename);
    let json_content = json!({
        "id": "test-id",
        "title": "Test Title",
        "description": "Test Description",
        "screenshot": screenshot_url
    });

    fs::write(&json_path, json_content.to_string())?;
    Ok(json_path)
}

// Helper function to create a JSON file without a screenshot URL
fn create_json_without_screenshot(dir: &Path, filename: &str) -> Result<PathBuf, Box<dyn Error>> {
    let json_path = dir.join(filename);
    let json_content = json!({
        "id": "test-id",
        "title": "Test Title",
        "description": "Test Description"
    });

    fs::write(&json_path, json_content.to_string())?;
    Ok(json_path)
}

// Helper function to create a JSON file with an empty screenshot URL
fn create_json_with_empty_screenshot(
    dir: &Path,
    filename: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let json_path = dir.join(filename);
    let json_content = json!({
        "id": "test-id",
        "title": "Test Title",
        "description": "Test Description",
        "screenshot": ""
    });

    fs::write(&json_path, json_content.to_string())?;
    Ok(json_path)
}

// Helper function to create a JSON file with "null" as screenshot URL
fn create_json_with_null_screenshot(dir: &Path, filename: &str) -> Result<PathBuf, Box<dyn Error>> {
    let json_path = dir.join(filename);
    let json_content = json!({
        "id": "test-id",
        "title": "Test Title",
        "description": "Test Description",
        "screenshot": "null"
    });

    fs::write(&json_path, json_content.to_string())?;
    Ok(json_path)
}

// Helper function to create test image data
fn create_test_png_data() -> Vec<u8> {
    // Simple PNG header + minimal data
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR chunk length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x00, 0x01, // width: 1
        0x00, 0x00, 0x00, 0x01, // height: 1
        0x08, // bit depth
        0x06, // color type
        0x00, // compression method
        0x00, // filter method
        0x00, // interlace method
        0x1F, 0x15, 0xC4, 0x89, // CRC
    ]
}

// Helper function to create test JPEG data
fn create_test_jpg_data() -> Vec<u8> {
    // Simple JPEG header + minimal data
    vec![
        0xFF, 0xD8, // SOI marker
        0xFF, 0xE0, // APP0 marker
        0x00, 0x10, // APP0 length
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
        0x01, 0x01, // version
        0x00, // units
        0x00, 0x01, // x density
        0x00, 0x01, // y density
        0x00, 0x00, // thumbnail
        0xFF, 0xD9, // EOI marker
    ]
}

#[test]
fn test_process_json_file_success() {
    // Set up temp directories
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create test image
    let test_image = create_test_png_data();

    // Set up a mock HTTP server
    let mut server = MockServer::new();
    let mock = server
        .mock("GET", "/test_image.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(&test_image)
        .create();

    // Create a test JSON file with a URL to the mock server
    let image_url = format!("{}/test_image.png", server.url());
    let json_path = create_test_json_file(&input_dir, "test.json", &image_url).unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result
    match result {
        FileProcessResult::Success => {
            // Check if image was saved
            let image_file_path = image_dir.join("test_image.png");
            assert!(image_file_path.exists(), "Image file should exist");

            // Check if base64 JSON was saved
            let base64_json_path = base64_dir.join("test.json");
            assert!(base64_json_path.exists(), "Base64 JSON file should exist");

            // Verify base64 JSON content
            let base64_json_content = fs::read_to_string(base64_json_path).unwrap();
            let json_value: Value = serde_json::from_str(&base64_json_content).unwrap();

            // Check if the screenshot field contains a data URL
            let screenshot = json_value["screenshot"].as_str().unwrap();
            assert!(
                screenshot.starts_with("data:image/png;base64,"),
                "Screenshot should be a PNG data URL"
            );

            // Extract and verify the base64-encoded image
            let base64_part = screenshot.split(',').nth(1).unwrap();
            let decoded = general_purpose::STANDARD.decode(base64_part).unwrap();
            assert_eq!(decoded, test_image, "Decoded image should match original");

            // Verify the mock was called
            mock.assert();
        }
        _ => panic!("Expected Success but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_missing_screenshot() {
    // Set up temp directories
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create a test JSON file without a screenshot URL
    let json_path = create_json_without_screenshot(&input_dir, "test_no_screenshot.json").unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result is Skipped
    match result {
        FileProcessResult::Skipped(_) => {
            // Success - file was skipped as expected
        }
        _ => panic!("Expected Skipped but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_empty_screenshot() {
    // Test with an empty screenshot URL
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    let json_path =
        create_json_with_empty_screenshot(&input_dir, "test_empty_screenshot.json").unwrap();
    let http_client = reqwest::blocking::Client::new();

    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    match result {
        FileProcessResult::Skipped(_) => {
            // Success - file was skipped as expected for empty URL
        }
        _ => panic!("Expected Skipped for empty URL but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_null_screenshot() {
    // Test with "null" as screenshot URL
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    let json_path =
        create_json_with_null_screenshot(&input_dir, "test_null_screenshot.json").unwrap();
    let http_client = reqwest::blocking::Client::new();

    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    match result {
        FileProcessResult::Skipped(_) => {
            // Success - file was skipped as expected for "null" URL
        }
        _ => panic!("Expected Skipped for 'null' URL but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_invalid_url() {
    // Test with an invalid URL
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create a test JSON file with an invalid URL
    let json_path =
        create_test_json_file(&input_dir, "test_invalid_url.json", "not_a_valid_url").unwrap();
    let http_client = reqwest::blocking::Client::new();

    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    match result {
        FileProcessResult::Failed(_, _) => {
            // Success - processing failed as expected for invalid URL
        }
        _ => panic!("Expected Failed for invalid URL but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_server_error() {
    // Test with a URL that returns a server error
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Set up a mock HTTP server that returns a 404
    let mut server = MockServer::new();
    let mock = server
        .mock("GET", "/non_existent.png")
        .with_status(404)
        .with_body("Not Found")
        .create();

    // Create a test JSON file with a URL to a non-existent resource
    let image_url = format!("{}/non_existent.png", server.url());
    let json_path =
        create_test_json_file(&input_dir, "test_server_error.json", &image_url).unwrap();
    let http_client = reqwest::blocking::Client::new();

    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    match result {
        FileProcessResult::Failed(_, _) => {
            // Success - processing failed as expected for server error
            mock.assert();
        }
        _ => panic!("Expected Failed for server error but got: {:?}", result),
    }
}

#[test]
fn test_process_json_file_different_image_types() {
    // Test with different image types (PNG, JPEG)
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create test images
    let png_image = create_test_png_data();
    let jpg_image = create_test_jpg_data();

    let mut server = MockServer::new();

    // Test PNG processing
    {
        let png_mock = server
            .mock("GET", "/test_png.png")
            .with_status(200)
            .with_header("content-type", "image/png")
            .with_body(&png_image)
            .create();

        let png_url = format!("{}/test_png.png", server.url());
        let json_path = create_test_json_file(&input_dir, "test_png.json", &png_url).unwrap();
        let http_client = reqwest::blocking::Client::new();

        let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

        match result {
            FileProcessResult::Success => {
                // Check if image was saved
                let image_file_path = image_dir.join("test_png.png");
                assert!(image_file_path.exists(), "PNG image file should exist");

                // Check if base64 JSON contains PNG data URL
                let base64_json_path = base64_dir.join("test_png.json");
                let base64_json_content = fs::read_to_string(base64_json_path).unwrap();
                let json_value: Value = serde_json::from_str(&base64_json_content).unwrap();

                let screenshot = json_value["screenshot"].as_str().unwrap();
                assert!(
                    screenshot.starts_with("data:image/png;base64,"),
                    "Screenshot should be a PNG data URL"
                );

                png_mock.assert();
            }
            _ => panic!("Expected Success for PNG processing but got: {:?}", result),
        }
    }

    // Test JPEG processing
    {
        let jpg_mock = server
            .mock("GET", "/test_jpg.jpg")
            .with_status(200)
            .with_header("content-type", "image/jpeg")
            .with_body(&jpg_image)
            .create();

        let jpg_url = format!("{}/test_jpg.jpg", server.url());
        let json_path = create_test_json_file(&input_dir, "test_jpg.json", &jpg_url).unwrap();
        let http_client = reqwest::blocking::Client::new();

        let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

        match result {
            FileProcessResult::Success => {
                // Check if image was saved
                let image_file_path = image_dir.join("test_jpg.jpg");
                assert!(image_file_path.exists(), "JPEG image file should exist");

                // Check if base64 JSON contains JPEG data URL
                let base64_json_path = base64_dir.join("test_jpg.json");
                let base64_json_content = fs::read_to_string(base64_json_path).unwrap();
                let json_value: Value = serde_json::from_str(&base64_json_content).unwrap();

                let screenshot = json_value["screenshot"].as_str().unwrap();
                assert!(
                    screenshot.starts_with("data:image/jpeg;base64,"),
                    "Screenshot should be a JPEG data URL"
                );

                jpg_mock.assert();
            }
            _ => panic!("Expected Success for JPEG processing but got: {:?}", result),
        }
    }
}

#[test]
fn test_url_filename_extraction() {
    // Test the URL filename extraction logic
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create test image
    let test_image = create_test_png_data();

    // Set up a mock HTTP server
    let mut server = MockServer::new();
    let mock = server
        .mock("GET", "/path/to/complex_filename.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(&test_image)
        .create();

    // Create a test JSON file with a URL that has a complex path
    let image_url = format!("{}/path/to/complex_filename.png", server.url());
    let json_path =
        create_test_json_file(&input_dir, "test_complex_path.json", &image_url).unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result
    match result {
        FileProcessResult::Success => {
            // Check that the filename was correctly extracted from the URL path
            let image_file_path = image_dir.join("complex_filename.png");
            assert!(
                image_file_path.exists(),
                "Image file should exist with correct name extracted from URL"
            );

            // Verify the content is correct
            let saved_image = fs::read(&image_file_path).unwrap();
            assert_eq!(
                saved_image, test_image,
                "Saved image content should match the original"
            );

            mock.assert();
        }
        _ => panic!("Expected Success but got: {:?}", result),
    }
}

#[test]
fn test_url_with_query_params() {
    // Test URL with query parameters
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create test image
    let test_image = create_test_png_data();

    // Set up a mock HTTP server
    let mut server = MockServer::new();
    let mock = server
        .mock("GET", "/image_with_params.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(&test_image)
        .match_query(mockito::Matcher::Any) // Match any query parameters
        .create();

    // Create a test JSON file with a URL that has query parameters
    let image_url = format!(
        "{}/image_with_params.png?width=800&height=600&token=abc123",
        server.url()
    );
    let json_path =
        create_test_json_file(&input_dir, "test_query_params.json", &image_url).unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result
    match result {
        FileProcessResult::Success => {
            // Check that the filename was correctly extracted from the URL (ignoring query params)
            let image_file_path = image_dir.join("image_with_params.png");
            assert!(
                image_file_path.exists(),
                "Image file should exist with correct name extracted from URL (without query params)"
            );
            mock.assert();
        }
        _ => panic!("Expected Success but got: {:?}", result),
    }
}

#[test]
fn test_malformed_json() {
    // Test with malformed JSON
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create malformed JSON file
    let json_path = input_dir.join("malformed.json");
    fs::write(
        &json_path,
        r#"{ "id": "test-id", "screenshot": "http://example.com/image.png" "#,
    )
    .unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result is Failed
    match result {
        FileProcessResult::Failed(_, error_msg) => {
            assert!(
                error_msg.contains("JSON Error") || error_msg.contains("Failed to parse JSON"),
                "Error should indicate JSON parsing issue, but got: {}",
                error_msg
            );
        }
        _ => panic!("Expected Failed for malformed JSON but got: {:?}", result),
    }
}

#[test]
fn test_non_utf8_filenames() {
    // This test is limited since Rust strings are UTF-8,
    // but we can test the handling of file names with special characters
    let temp_dir = tempdir().unwrap();
    let input_dir = temp_dir.path().join("input");
    let image_dir = temp_dir.path().join("images");
    let base64_dir = temp_dir.path().join("base64");

    fs::create_dir_all(&input_dir).unwrap();
    fs::create_dir_all(&image_dir).unwrap();
    fs::create_dir_all(&base64_dir).unwrap();

    // Create test image
    let test_image = create_test_png_data();

    // Set up a mock HTTP server
    let mut server = MockServer::new();
    let mock = server
        .mock("GET", "/image.png")
        .with_status(200)
        .with_header("content-type", "image/png")
        .with_body(&test_image)
        .create();

    // Create a test JSON file with a special character filename
    let special_filename = "test_特殊字符.json";
    let image_url = format!("{}/image.png", server.url());
    let json_path = create_test_json_file(&input_dir, special_filename, &image_url).unwrap();

    // Create HTTP client
    let http_client = reqwest::blocking::Client::new();

    // Process the JSON file
    let result = process_json_file(&json_path, &image_dir, &base64_dir, &http_client, false);

    // Verify the result
    match result {
        FileProcessResult::Success => {
            // Check if image was saved
            let image_file_path = image_dir.join("image.png");
            assert!(image_file_path.exists(), "Image file should exist");

            // Check if base64 JSON was saved with the special character filename
            let base64_json_path = base64_dir.join(special_filename);
            assert!(
                base64_json_path.exists(),
                "Base64 JSON file should exist with special characters in filename"
            );

            mock.assert();
        }
        _ => panic!("Expected Success but got: {:?}", result),
    }
}
