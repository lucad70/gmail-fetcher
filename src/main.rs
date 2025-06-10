use rustls;
use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Gmail IMAP Email Fetcher");
    println!("========================");

    // Step 0: Get user credentials
    print!("Enter your Gmail address: ");
    io::stdout().flush()?;
    let mut email = String::new();
    io::stdin().read_line(&mut email)?;
    let email = email.trim().to_string();

    print!("Enter your app password: ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let password = password.trim().to_string();

    // Step 1: Get directory path and create if it doesn't exist
    print!("Enter absolute path for saving emails: ");
    io::stdout().flush()?;
    let mut dir_path = String::new();
    io::stdin().read_line(&mut dir_path)?;
    let dir_path = dir_path.trim();

    if !Path::new(dir_path).exists() {
        println!("Directory doesn't exist. Creating: {}", dir_path);
        fs::create_dir_all(dir_path)?;
    } else {
        println!("Directory exists: {}", dir_path);
    }

    // Step 2: Create TLS connection to Gmail IMAP
    println!("Connecting to imap.gmail.com:993...");

    // Establish TCP connection
    let tcp_stream = TcpStream::connect("imap.gmail.com:993")?;

    // Set up TLS configuration
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let server_name = "imap.gmail.com".try_into()?;
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name)?;
    let mut tls_stream = rustls::StreamOwned::new(conn, tcp_stream);

    // Read initial server greeting
    let mut buffer = [0; 1024];
    let n = tls_stream.read(&mut buffer)?;
    let greeting = String::from_utf8_lossy(&buffer[..n]);
    println!("Server greeting: {}", greeting.trim());

    // Step 3: Send LOGIN command
    println!("Authenticating...");
    let login_cmd = format!("A001 LOGIN {} {}\r\n", email, password);
    tls_stream.write_all(login_cmd.as_bytes())?;
    tls_stream.flush()?;

    // Read LOGIN response
    let mut response_buffer = Vec::new();
    loop {
        let mut byte = [0; 1];
        tls_stream.read_exact(&mut byte)?;
        response_buffer.push(byte[0]);

        if response_buffer.len() >= 2
            && response_buffer[response_buffer.len() - 2] == b'\r'
            && response_buffer[response_buffer.len() - 1] == b'\n'
        {
            let response = String::from_utf8_lossy(&response_buffer);
            println!("LOGIN response: {}", response.trim());

            if response.starts_with("A001") {
                if response.contains("OK") {
                    println!("Authentication successful!");
                    break;
                } else {
                    return Err("Authentication failed".into());
                }
            }
            response_buffer.clear();
        }
    }

    // Step 4: Send SELECT INBOX command
    println!("Selecting INBOX...");
    let select_cmd = "A002 SELECT INBOX\r\n";
    tls_stream.write_all(select_cmd.as_bytes())?;
    tls_stream.flush()?;

    let mut email_count = 0;
    // Read SELECT response
    loop {
        let mut byte = [0; 1];
        tls_stream.read_exact(&mut byte)?;
        response_buffer.push(byte[0]);

        if response_buffer.len() >= 2
            && response_buffer[response_buffer.len() - 2] == b'\r'
            && response_buffer[response_buffer.len() - 1] == b'\n'
        {
            let response = String::from_utf8_lossy(&response_buffer);
            println!("SELECT response: {}", response.trim());

            // Parse email count from "* XXXX EXISTS" line
            if response.contains("EXISTS") {
                let parts: Vec<&str> = response.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(count) = parts[1].parse::<u32>() {
                        email_count = count;
                        println!("Found {} emails in INBOX", email_count);
                    }
                }
            }

            if response.starts_with("A002") {
                if response.contains("OK") {
                    println!("INBOX selected successfully!");
                    break;
                } else {
                    return Err("Failed to select INBOX".into());
                }
            }
            response_buffer.clear();
        }
    }

    if email_count == 0 {
        println!("No emails found in INBOX");
        return Ok(());
    }

    // Step 5: Fetch emails in batches
    let batch_size = 10;
    let mut command_id = 3;
    let mut total_fetched = 0;

    // Adjust this limit as needed - remove to fetch all emails
    //let max_emails = std::cmp::min(email_count, 100);
    println!(
        "Fetching first {} emails in batches of {}...",
        email_count, batch_size
    );

    for start in (1..=email_count).step_by(batch_size as usize) {
        let end = std::cmp::min(start + batch_size - 1, email_count);
        println!("Fetching emails {} to {}...", start, end);

        let fetch_cmd = format!("A{:03} FETCH {}:{} (BODY[])\r\n", command_id, start, end);
        tls_stream.write_all(fetch_cmd.as_bytes())?;
        tls_stream.flush()?;

        // Process this batch
        total_fetched += process_batch(&mut tls_stream, command_id, dir_path)?;
        command_id += 1;

        // Small delay between batches to be nice to the server
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("Total emails fetched: {}", total_fetched);

    // Step 6: Send LOGOUT command
    println!("Logging out...");
    let logout_cmd = "A999 LOGOUT\r\n";
    tls_stream.write_all(logout_cmd.as_bytes())?;
    tls_stream.flush()?;

    // Read LOGOUT response
    let n = tls_stream.read(&mut buffer)?;
    let logout_response = String::from_utf8_lossy(&buffer[..n]);
    println!("LOGOUT response: {}", logout_response.trim());

    println!(
        "Email fetching completed! All emails saved to: {}",
        dir_path
    );
    Ok(())
}

fn process_batch(
    tls_stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    command_id: u32,
    dir_path: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let mut response_buffer = Vec::new();
    let mut current_email_data = Vec::new();
    let mut reading_email_body = false;
    let mut email_body_size = 0;
    let mut body_bytes_read = 0;
    let mut current_email_id = 0;
    let mut emails_saved = 0;
    let mut expecting_closing_paren = false;

    loop {
        let mut buffer = [0; 4096];
        match tls_stream.read(&mut buffer) {
            Ok(0) => {
                println!("Connection closed unexpectedly");
                break;
            }
            Ok(n) => {
                for i in 0..n {
                    let byte = buffer[i];

                    if reading_email_body {
                        // We're reading the email body content
                        current_email_data.push(byte);
                        body_bytes_read += 1;

                        if body_bytes_read >= email_body_size {
                            // We've read the complete email body
                            let filename =
                                format!("{}/email_{:05}.eml", dir_path, current_email_id);
                            fs::write(&filename, &current_email_data)?;
                            println!("Saved email {} to {}", current_email_id, filename);

                            emails_saved += 1;
                            reading_email_body = false;
                            expecting_closing_paren = true;
                            current_email_data.clear();
                        }
                    } else if expecting_closing_paren {
                        // Skip characters until we see the closing parenthesis and CRLF
                        if byte == b')' {
                            expecting_closing_paren = false;
                        }
                        // Continue to next byte without adding to response_buffer
                    } else {
                        // We're reading response lines
                        response_buffer.push(byte);

                        // Check for complete line
                        if response_buffer.len() >= 2
                            && response_buffer[response_buffer.len() - 2] == b'\r'
                            && response_buffer[response_buffer.len() - 1] == b'\n'
                        {
                            let line = String::from_utf8_lossy(&response_buffer);
                            let line_str = line.trim();

                            // Check for FETCH response with email ID and body size
                            if line_str.contains("FETCH") && line_str.contains("{") {
                                // Extract email ID from "* ID FETCH ..."
                                if let Some(fetch_start) = line_str.find("* ") {
                                    if let Some(fetch_end) = line_str.find(" FETCH") {
                                        if let Ok(id) =
                                            line_str[fetch_start + 2..fetch_end].parse::<u32>()
                                        {
                                            current_email_id = id;
                                        }
                                    }
                                }

                                // Extract body size from "{SIZE}"
                                if let Some(size_start) = line_str.find("{") {
                                    if let Some(size_end) = line_str.find("}") {
                                        if let Ok(size) =
                                            line_str[size_start + 1..size_end].parse::<usize>()
                                        {
                                            email_body_size = size;
                                            body_bytes_read = 0;
                                            reading_email_body = true;
                                            current_email_data.clear();
                                            println!(
                                                "Reading email {} (size: {} bytes)",
                                                current_email_id, size
                                            );
                                        }
                                    }
                                }
                            } else if line_str.starts_with(&format!("A{:03}", command_id)) {
                                // This is the completion response for our command
                                if line_str.contains("OK") {
                                    println!("Batch completed successfully");
                                    return Ok(emails_saved);
                                } else if line_str.contains("BAD") || line_str.contains("NO") {
                                    eprintln!("FETCH failed: {}", line_str);
                                    return Err(
                                        format!("FETCH command failed: {}", line_str).into()
                                    );
                                }
                            }

                            response_buffer.clear();
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading from stream: {}", e);
                return Err(e.into());
            }
        }
    }

    Ok(emails_saved)
}

// Add these dependencies to Cargo.toml:
/*
[dependencies]
rustls = "0.22"
webpki-roots = "0.26"
*/
