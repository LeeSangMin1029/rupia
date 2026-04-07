#[derive(thiserror::Error, Debug)]
pub enum EmbedError {
    #[error("model initialization failed: {0}")]
    ModelInit(String),
    #[error("embedding generation failed: {0}")]
    EmbeddingFailed(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
}
