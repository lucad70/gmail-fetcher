use crate::error_imap::ClientError;
use std::io::{self};
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
            max_concurrent: Self::determine_optimal_concurrency(),
        }
    }
    fn determine_optimal_concurrency() -> usize {
        //if let Ok(parallelism) = std::thread::available_parallelism() {
        //return 2 * parallelism.get();
        //}
        5
    }
}

pub fn prompt_imap_config() -> Result<ImapConfig, ClientError> {
    let mut config = ImapConfig::new();

    config.email = prompt_email()?;
    config.password = prompt_password()?;
    config.dir_path = prompt_directory_path()?;

    Ok(config)
}

pub fn prompt_email() -> Result<String, ClientError> {
    println!("Enter your Gmail address: ");
    let input = get_user_input()?;
    validate_email(&input)?;
    Ok(input)
}

pub fn prompt_password() -> Result<String, ClientError> {
    println!("Enter your app password: ");
    let input = get_user_input()?;
    if input.is_empty() {
        return Err(ClientError::EmptyInput {
            field: "password".to_string(),
        });
    }
    Ok(input)
}

pub fn prompt_directory_path() -> Result<String, ClientError> {
    println!("Enter absolute path for saving emails: ");
    let dir_path = get_user_input()?;

    if !Path::new(&dir_path).exists() {
        log::info!("Directory doesn't exist. Creating: {}", dir_path);
        std::fs::create_dir_all(&dir_path)?;
    } else {
        log::info!("Directory exists: {}", dir_path);
    }

    Ok(dir_path)
}

fn get_user_input() -> Result<String, ClientError> {
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let mut trimmed = input.trim().to_string();
    trimmed = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
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
