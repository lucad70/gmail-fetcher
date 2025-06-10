use rustls;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tokio_rustls::{client::TlsStream, TlsConnector};

use crate::error_imap::ClientError;
use crate::input::ImapConfig;

pub struct ImapClient {
    config: ImapConfig,
}

impl ImapClient {
    pub fn new(config: ImapConfig) -> Self {
        ImapClient { config }
    }

    pub async fn fetch_all_emails(&self) -> Result<(), ClientError> {
        println!("Gmail IMAP Email Fetcher (Async Version)");
        println!("========================================");

        println!(
            "Using {} concurrent connections",
            self.config.max_concurrent
        );

        // Step 1: Get email count
        let email_count = self.get_email_count().await?;

        if email_count == 0 {
            println!("No emails found in INBOX");
            return Ok(());
        }

        println!("Found {} emails in INBOX", email_count);

        // Step 2: Fetch emails concurrently
        self.fetch_emails_concurrently(email_count).await?;

        println!(
            "Email fetching completed! All emails saved to: {}",
            self.config.dir_path
        );
        Ok(())
    }

    async fn get_email_count(&self) -> Result<u32, ClientError> {
        println!("Connecting to get email count...");

        let mut tls_stream = create_tls_connection().await?;
        authenticate(&mut tls_stream, &self.config.email, &self.config.password).await?;

        // Send SELECT INBOX command
        let select_cmd = "A002 SELECT INBOX\r\n";
        tls_stream
            .write_all(select_cmd.as_bytes())
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        tls_stream
            .flush()
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

        let mut email_count = 0;
        let mut response_buffer = Vec::new();

        loop {
            let mut byte = [0; 1];
            tls_stream
                .read_exact(&mut byte)
                .await
                .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
            response_buffer.push(byte[0]);

            if response_buffer.len() >= 2
                && response_buffer[response_buffer.len() - 2] == b'\r'
                && response_buffer[response_buffer.len() - 1] == b'\n'
            {
                let response = String::from_utf8_lossy(&response_buffer);

                // Parse email count from "* XXXX EXISTS" line
                if response.contains("EXISTS") {
                    let parts: Vec<&str> = response.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(count) = parts[1].parse::<u32>() {
                            email_count = count;
                        }
                    }
                }

                if response.starts_with("A002") {
                    if response.contains("OK") {
                        break;
                    } else {
                        return Err(ClientError::ImapError("Failed to select INBOX".to_string()));
                    }
                }
                response_buffer.clear();
            }
        }

        // Logout
        let logout_cmd = "A999 LOGOUT\r\n";
        tls_stream
            .write_all(logout_cmd.as_bytes())
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        tls_stream
            .flush()
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

        Ok(email_count)
    }

    async fn fetch_emails_concurrently(&self, email_count: u32) -> Result<(), ClientError> {
        let batch_size = 10;
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent));
        let mut handles = Vec::new();

        println!(
            "Fetching emails in batches of {} with {} concurrent connections...",
            batch_size, self.config.max_concurrent
        );

        for start in (1..=email_count).step_by(batch_size as usize) {
            let end = std::cmp::min(start + batch_size - 1, email_count);

            let semaphore = Arc::clone(&semaphore);
            let email = self.config.email.clone();
            let password = self.config.password.clone();
            let dir_path = self.config.dir_path.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                match fetch_email_batch(start, end, &email, &password, &dir_path).await {
                    Ok(count) => {
                        println!(
                            "Successfully fetched emails {} to {} ({} emails)",
                            start, end, count
                        );
                        Ok::<u32, String>(count)
                    }
                    Err(e) => {
                        eprintln!("Failed to fetch emails {} to {}: {}", start, end, e);
                        Err(e.to_string())
                    }
                }
            });

            handles.push(handle);

            // Small delay to avoid overwhelming the server
            sleep(Duration::from_millis(50)).await;
        }

        // Wait for all batches to complete
        let mut total_fetched = 0;
        let mut errors = 0;

        for handle in handles {
            match handle.await {
                Ok(Ok(count)) => total_fetched += count,
                Ok(Err(_)) => errors += 1,
                Err(e) => {
                    eprintln!("Task join error: {}", e);
                    errors += 1;
                }
            }
        }

        println!("Total emails fetched: {}", total_fetched);
        if errors > 0 {
            println!("Encountered {} errors during fetching", errors);
        }

        Ok(())
    }
}

async fn fetch_email_batch(
    start: u32,
    end: u32,
    email: &str,
    password: &str,
    dir_path: &str,
) -> Result<u32, ClientError> {
    let mut tls_stream = create_tls_connection().await?;
    authenticate(&mut tls_stream, email, password).await?;
    select_inbox(&mut tls_stream).await?;

    // Fetch emails in this batch
    let fetch_cmd = format!("A003 FETCH {}:{} (BODY[])\r\n", start, end);
    tls_stream
        .write_all(fetch_cmd.as_bytes())
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
    tls_stream
        .flush()
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    let emails_saved = process_batch_async(&mut tls_stream, dir_path).await?;

    // Logout
    let logout_cmd = "A999 LOGOUT\r\n";
    tls_stream
        .write_all(logout_cmd.as_bytes())
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
    tls_stream
        .flush()
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    Ok(emails_saved)
}

async fn create_tls_connection() -> Result<TlsStream<TcpStream>, ClientError> {
    // Establish TCP connection
    let tcp_stream = TcpStream::connect("imap.gmail.com:993")
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    // Set up TLS configuration
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from("imap.gmail.com")?;
    let tls_stream = connector
        .connect(server_name, tcp_stream)
        .await
        .map_err(|e| ClientError::TlsError(e.to_string()))?;

    Ok(tls_stream)
}

async fn authenticate(
    tls_stream: &mut TlsStream<TcpStream>,
    email: &str,
    password: &str,
) -> Result<(), ClientError> {
    // Read initial server greeting
    let mut buffer = [0; 1024];
    let _n = tls_stream
        .read(&mut buffer)
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    // Send LOGIN command
    let login_cmd = format!("A001 LOGIN {} {}\r\n", email, password);
    tls_stream
        .write_all(login_cmd.as_bytes())
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
    tls_stream
        .flush()
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    // Read LOGIN response
    let mut response_buffer = Vec::new();
    loop {
        let mut byte = [0; 1];
        tls_stream
            .read_exact(&mut byte)
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        response_buffer.push(byte[0]);

        if response_buffer.len() >= 2
            && response_buffer[response_buffer.len() - 2] == b'\r'
            && response_buffer[response_buffer.len() - 1] == b'\n'
        {
            let response = String::from_utf8_lossy(&response_buffer);

            if response.starts_with("A001") {
                if response.contains("OK") {
                    return Ok(());
                } else {
                    return Err(ClientError::AuthenticationError(
                        "Authentication failed".to_string(),
                    ));
                }
            }
            response_buffer.clear();
        }
    }
}

async fn select_inbox(tls_stream: &mut TlsStream<TcpStream>) -> Result<(), ClientError> {
    let select_cmd = "A002 SELECT INBOX\r\n";
    tls_stream
        .write_all(select_cmd.as_bytes())
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
    tls_stream
        .flush()
        .await
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;

    let mut response_buffer = Vec::new();
    loop {
        let mut byte = [0; 1];
        tls_stream
            .read_exact(&mut byte)
            .await
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        response_buffer.push(byte[0]);

        if response_buffer.len() >= 2
            && response_buffer[response_buffer.len() - 2] == b'\r'
            && response_buffer[response_buffer.len() - 1] == b'\n'
        {
            let response = String::from_utf8_lossy(&response_buffer);

            if response.starts_with("A002") {
                if response.contains("OK") {
                    return Ok(());
                } else {
                    return Err(ClientError::ImapError("Failed to select INBOX".to_string()));
                }
            }
            response_buffer.clear();
        }
    }
}

async fn process_batch_async(
    tls_stream: &mut TlsStream<TcpStream>,
    dir_path: &str,
) -> Result<u32, ClientError> {
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
        match tls_stream.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                for i in 0..n {
                    let byte = buffer[i];

                    if reading_email_body {
                        current_email_data.push(byte);
                        body_bytes_read += 1;

                        if body_bytes_read >= email_body_size {
                            // Save email
                            let filename =
                                format!("{}/email_{:05}.eml", dir_path, current_email_id);
                            tokio::fs::write(&filename, &current_email_data)
                                .await
                                .map_err(|e| ClientError::FileError(e.to_string()))?;
                            println!("Saved email {} to {}", current_email_id, filename);

                            emails_saved += 1;
                            reading_email_body = false;
                            expecting_closing_paren = true;
                            current_email_data.clear();
                        }
                    } else if expecting_closing_paren {
                        if byte == b')' {
                            expecting_closing_paren = false;
                        }
                    } else {
                        response_buffer.push(byte);

                        if response_buffer.len() >= 2
                            && response_buffer[response_buffer.len() - 2] == b'\r'
                            && response_buffer[response_buffer.len() - 1] == b'\n'
                        {
                            let line = String::from_utf8_lossy(&response_buffer);
                            let line_str = line.trim();

                            if line_str.contains("FETCH") && line_str.contains("{") {
                                // Extract email ID
                                if let Some(fetch_start) = line_str.find("* ") {
                                    if let Some(fetch_end) = line_str.find(" FETCH") {
                                        if let Ok(id) =
                                            line_str[fetch_start + 2..fetch_end].parse::<u32>()
                                        {
                                            current_email_id = id;
                                        }
                                    }
                                }

                                // Extract body size
                                if let Some(size_start) = line_str.find("{") {
                                    if let Some(size_end) = line_str.find("}") {
                                        if let Ok(size) =
                                            line_str[size_start + 1..size_end].parse::<usize>()
                                        {
                                            email_body_size = size;
                                            body_bytes_read = 0;
                                            reading_email_body = true;
                                            current_email_data.clear();
                                        }
                                    }
                                }
                            } else if line_str.starts_with("A003") {
                                if line_str.contains("OK") {
                                    return Ok(emails_saved);
                                } else if line_str.contains("BAD") || line_str.contains("NO") {
                                    return Err(ClientError::ImapError(format!(
                                        "FETCH command failed: {}",
                                        line_str
                                    )));
                                }
                            }

                            response_buffer.clear();
                        }
                    }
                }
            }
            Err(e) => return Err(ClientError::ConnectionError(e.to_string())),
        }
    }

    Ok(emails_saved)
}
