use crate::error_imap::ClientError;
use std::io::{self, Write};
use std::path::Path;

pub struct ImapConfig {
    pub email: String,
    pub password: String,
    pub dir_path: String,
    pub max_concurrent: usize,
}

impl ImapConfig {
    pub fn new() -> Self {
        ImapConfig {
            email: String::new(),
            password: String::new(),
            dir_path: String::new(),
            max_concurrent: 5,
        }
    }
}

pub async fn prompt_imap_config() -> Result<ImapConfig, ClientError> {
    let mut config = ImapConfig::new();

    config.email = prompt_email()?;
    config.password = prompt_password()?;
    config.dir_path = prompt_directory_path().await?;

    Ok(config)
}

pub fn prompt_email() -> Result<String, ClientError> {
    print!("Enter your Gmail address: ");
    io::stdout().flush()?;
    let input = get_user_input()?;
    validate_email(&input)?;
    Ok(input)
}

pub fn prompt_password() -> Result<String, ClientError> {
    print!("Enter your app password: ");
    io::stdout().flush()?;
    let input = get_user_input()?;
    if input.is_empty() {
        return Err(ClientError::EmptyInput {
            field: "password".to_string(),
        });
    }
    Ok(input)
}

pub async fn prompt_directory_path() -> Result<String, ClientError> {
    print!("Enter absolute path for saving emails: ");
    io::stdout().flush().map_err(ClientError::InputError)?;
    let dir_path = get_user_input()?;

    if !Path::new(&dir_path).exists() {
        println!("Directory doesn't exist. Creating: {}", dir_path);
        tokio::fs::create_dir_all(&dir_path)
            .await
            .map_err(|e| ClientError::DirectoryError(e.to_string()))?;
    } else {
        println!("Directory exists: {}", dir_path);
    }

    Ok(dir_path)
}

fn get_user_input() -> Result<String, ClientError> {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(ClientError::InputError)?;

    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        return Err(ClientError::EmptyInput {
            field: "input".to_string(),
        });
    }

    Ok(trimmed)
}

fn validate_email(email: &str) -> Result<(), ClientError> {
    if email.is_empty() {
        return Err(ClientError::EmptyInput {
            field: "email".to_string(),
        });
    }

    // Basic email validation
    if !email.contains('@') || !email.contains('.') {
        return Err(ClientError::InputError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid email format",
        )));
    }

    Ok(())
}
