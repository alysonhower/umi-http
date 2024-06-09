use anyhow::{anyhow, Result};
use clap::Parser;
use regex::Regex;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::fs;
use std::path::PathBuf;
use std::process;
use std::time::Duration;
use tokio::time::sleep;

const URL: &str = "http://127.0.0.1:1224/argv";
const TAB_NAME: &str = "BatchDOC";
const MAX_ATTEMPTS: u8 = 3;
const DELAY: Duration = Duration::from_secs(1);

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    path: String,
}

async fn send_request(data: Value) -> Result<String> {
    let client = Client::new();
    let response = client
        .post(URL)
        .header("Content-Type", "application/json")
        .json(&data)
        .send()
        .await
        .map_err(|error| anyhow!("Error sending request to Umi-OCR: {}", error))?;
    response
        .text()
        .await
        .map_err(|error| anyhow!("Error reading response from Umi-OCR: {}", error))
}

async fn tabs() -> Result<String> {
    send_request(json!(["--all_pages"])).await
}

async fn open_batch_ocr() -> Result<()> {
    println!("Opening Batch OCR...");
    send_request(json!(["--add_page", "3"])).await?;
    println!("Batch OCR opened.");
    Ok(())
}

async fn close_batch_ocr(index: u16) -> Result<()> {
    println!("Closing Batch OCR with index {}...", index);
    send_request(json!(["--del_page", index.to_string()])).await?;
    println!("Batch OCR with index {} closed.", index);
    Ok(())
}

async fn add_docs(path: &str) -> Result<()> {
    println!("Adding document from path {}...", path);
    send_request(json!([
        "--call_qml",
        "BatchDOC",
        "--func",
        "addDocs",
        format!("[\"{}\"]", path)
    ]))
    .await?;
    println!("Documents added.");
    Ok(())
}

async fn doc_start() -> Result<()> {
    println!("Starting document processing...");
    send_request(json!(["--call_qml", "BatchDOC", "--func", "docStart"])).await?;
    println!("Document processing started.");
    Ok(())
}

async fn verify() -> Result<()> {
    let regex = Regex::new(&format!(r"{}_\d+", TAB_NAME))?;
    for attempt in 1..=MAX_ATTEMPTS {
        if regex.find(&tabs().await?).is_some() {
            println!("{} found on attempt {}.", TAB_NAME, attempt);
            return Ok(());
        }
        println!("{} not found on attempt {}. Retrying...", TAB_NAME, attempt);
        sleep(DELAY).await;
    }
    Err(anyhow!(
        "Max attempts reached for {}. Tab now found.",
        TAB_NAME
    ))
}

async fn watch_output(path: PathBuf) -> Result<()> {
    if path.exists() {
        let metadata = fs::metadata(&path).await?;
        let last_modified = metadata.modified()?;

        loop {
            sleep(DELAY).await;
            let metadata = fs::metadata(&path).await?;
            let current_modified = metadata.modified()?;
            if current_modified != last_modified {
                println!("Document at path: {} has been overwritten", path.display());
                break;
            }
        }
    } else {
        println!("Waiting for document to exist at path: {}", path.display());
        while !path.exists() {
            sleep(DELAY).await;
        }
        println!("Document detected at path: {}", path.display());
    }
    Ok(())
}

async fn run(path: &str) -> Result<()> {
    let re = Regex::new(r"(?m)^(\d+)\s+BatchDOC_").unwrap();
    let indices: Vec<u16> = re
        .captures_iter(&tabs().await?)
        .filter_map(|cap| cap.get(1).and_then(|index| index.as_str().parse().ok()))
        .collect();

    for index in indices.into_iter().rev() {
        close_batch_ocr(index).await?;
        sleep(DELAY).await;
    }

    open_batch_ocr().await?;
    sleep(DELAY).await;
    verify().await?;

    let path = path.replace("\\", "/");

    add_docs(&path).await?;
    sleep(DELAY).await;

    doc_start().await?;

    let path = PathBuf::from(path);
    let path_rm_ext = path.with_extension("");
    let file_name = path_rm_ext.file_name().unwrap().to_string_lossy();
    let output_path = path.with_file_name(format!("{}.layered.pdf", file_name));
    let path = output_path.to_str().unwrap().replace("\\", "/");

    watch_output(PathBuf::from(path)).await
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(error) = run(&args.path).await {
        eprintln!("Error: {}", error);
        process::exit(1)
    }
    println!("Done!");
    process::exit(0)
}
