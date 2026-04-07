use crate::embed::error::EmbedError;

pub type Result<T> = std::result::Result<T, EmbedError>;

pub trait EmbeddingModel: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn embed_query(&self, query: &str) -> Result<Vec<f32>>;
    fn dim(&self) -> usize;
    fn name(&self) -> &str;
}
