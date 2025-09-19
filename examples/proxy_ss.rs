//! This example shows how to use a Shadowsocks proxy with reqwest.
//!
//! It reads a list of `ss://` URLs from `~/.config/ss`, and then
//! sequentially tests each proxy by making a request to a public IP echo service.
//!
//! The test results, including the response status and IP address, are printed to the console.

#![allow(clippy::manual_flatten)]

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use anyhow::Context;
use reqwest::{Client, Proxy, StatusCode};
use url::Url;
use percent_encoding::percent_decode_str;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Set the log level to "warn" to avoid overly verbose output.
    std::env::set_var("RUST_LOG", "warn");
    env_logger::init();

    println!("Testing Shadowsocks proxies...");

    // The URL to use for checking the public IP address.
    let test_url = "https://ifconfig.me/ip";

    let proxy_ss_urls = find_proxy_ss_urls()?;

    if proxy_ss_urls.is_empty() {
        println!("No ss:// proxy found in ~/.config/ss");
        println!("Please add Shadowsocks proxy URLs to the ~/.config/ss file.");
        return Ok(());
    }

    // Get the real IP address (without using any proxy).
    let direct_client = Client::builder().no_proxy().build()?;
    let real_ip = match get_ip_with_status(&direct_client, test_url).await {
        Ok((ip, _)) if !ip.is_empty() && !ip.contains("Error") => {
            println!("Real IP: {}", ip);
            ip
        }
        _ => {
            println!("Failed to get real IP address.");
            return Ok(());
        }
    };

    // Test each proxy sequentially.
    for (proxy_idx, proxy_url) in proxy_ss_urls.iter().enumerate() {
        println!("
Testing proxy {} / {}...", proxy_idx + 1, proxy_ss_urls.len());

        // Parse the proxy URL to get basic information for display.
        let parsed_url = match Url::parse(proxy_url) {
            Ok(url) => url,
            Err(e) => {
                println!("Failed to parse proxy URL: {}", e);
                continue;
            }
        };

        let server = parsed_url.host_str().unwrap_or("unknown");
        let port = parsed_url.port().unwrap_or(0);
        let name = parsed_url.fragment().unwrap_or("");
        // Explicitly URL-decode the name from the fragment.
        let decoded_name = percent_decode_str(name).decode_utf8_lossy();
        println!("{}:{} {}", server, port, decoded_name);

        let proxy = match Proxy::all(proxy_url) {
            Ok(proxy) => proxy,
            Err(e) => {
                println!("Failed to create proxy: {}", e);
                continue;
            }
        };

        let client = match Client::builder()
            .proxy(proxy)
            .danger_accept_invalid_certs(true) // Needed for some proxy setups.
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                println!("Failed to build client with proxy: {}", e);
                continue;
            }
        };

        // Test the proxy connection by making a request.
        let test_result = get_ip_with_status(&client, test_url).await;

        // Print the test results.
        match test_result {
            Ok((proxy_ip, status)) => {
                println!(
                    "Status: {} | Proxy IP: {} | Real IP: {}",
                    status, proxy_ip, real_ip
                );

                if proxy_ip == real_ip {
                    println!("Result: FAILED - Proxy not working (IP is the same as real IP)");
                } else {
                    println!("Result: SUCCESS - Proxy is working (IP has changed)");
                }
            }
            Err(e) => {
                println!("Result: FAILED - The request failed: {:?}", e);
            }
        }
    }

    Ok(())
}

/// Finds ss:// URLs in the ~/.config/ss file.
fn find_proxy_ss_urls() -> anyhow::Result<Vec<String>> {
    let mut path = PathBuf::from(std::env::var("HOME")?);
    path.push(".config/ss");

    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut urls = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.starts_with("ss://") {
            urls.push(line);
        }
    }

    Ok(urls)
}

async fn get_ip_with_status(client: &Client, url: &str) -> anyhow::Result<(String, StatusCode)> {
    let res = client.get(url).send().await.context("Failed to send request")?;
    let status = res.status();
    let body = res.text().await.context("Failed to read response body")?;
    Ok((body.trim().to_string(), status))
}
