use crate::embed::error::EmbedError;
use crate::embed::mmap_model::MmapStaticModel;
use crate::embed::model::{EmbeddingModel, Result};

const DEFAULT_MODEL: &str = "minishlab/potion-base-32M";

pub struct Model2VecModel {
    inner: MmapStaticModel,
}

impl Model2VecModel {
    pub fn new() -> Result<Self> {
        Self::from_pretrained(DEFAULT_MODEL)
    }
    pub fn from_pretrained(model_name: &str) -> Result<Self> {
        let inner = MmapStaticModel::from_pretrained(model_name)?;
        Ok(Self { inner })
    }
}

impl EmbeddingModel for Model2VecModel {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Err(EmbedError::InvalidInput("empty text list".into()));
        }
        self.inner.encode_batch(texts)
    }
    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        if query.is_empty() {
            return Err(EmbedError::InvalidInput("empty query".into()));
        }
        self.inner.encode_single(query)
    }
    fn dim(&self) -> usize {
        self.inner.dim()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
}
