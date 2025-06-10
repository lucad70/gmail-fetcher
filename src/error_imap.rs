use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Failed to read user input: {0}")]
    InputError(#[from] std::io::Error),

    #[error("Empty input provided for {field}")]
    EmptyInput { field: String },

    #[error("User cancelled operation")]
    UserCancelled,

    #[error("IMAP server responded with error: {0}")]
    ImapError(String),

    #[error("TLS error: {0}")]
    TlsError(String),

    #[error("Failed to connect to IMAP server: {0}")]
    ConnectionError(String),

    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Invalid DNS name: {0}")]
    InvalidDnsName(#[from] rustls::pki_types::InvalidDnsNameError),

    #[error("Failed to establish TLS connection")]
    TlsConnectionFailed(#[from] rustls::Error),

    #[error("Failed to parse email count")]
    ParseError,

    #[error("Directory creation failed: {0}")]
    DirectoryError(String),

    #[error("File operation failed: {0}")]
    FileError(String),

    #[error("Join error: {0}")]
    JoinError(String),
}
