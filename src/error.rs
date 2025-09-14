use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Audio error: {0}")]
    Audio(String),
    
    #[error("Stream error: {0}")]
    Stream(String),
    
    #[error("Station error: {0}")]
    Station(String),
    
    #[error("UDP error: {0}")]
    Udp(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Generic error: {0}")]
    Generic(String),
}