#[cfg(feature = "semantic")]
pub mod error;
#[cfg(feature = "semantic")]
pub mod mmap_model;
#[cfg(feature = "semantic")]
pub mod model;
#[cfg(feature = "semantic")]
pub mod model2vec;

#[cfg(feature = "semantic")]
use model::EmbeddingModel;
#[cfg(feature = "semantic")]
use model2vec::Model2VecModel;

pub fn cosine_f32(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    f64::from(dot / (na * nb))
}

#[cfg(feature = "semantic")]
static SEMANTIC_INDEX: std::sync::OnceLock<Option<SemanticIndex>> = std::sync::OnceLock::new();

#[cfg(feature = "semantic")]
pub struct SemanticIndex {
    model: Model2VecModel,
}

#[cfg(feature = "semantic")]
impl SemanticIndex {
    pub fn global() -> Option<&'static Self> {
        SEMANTIC_INDEX
            .get_or_init(|| {
                let model = Model2VecModel::from_pretrained("minishlab/potion-base-32M").ok()?;
                Some(Self { model })
            })
            .as_ref()
    }
    pub fn embed(&self, text: &str) -> Vec<f32> {
        self.model.embed_query(text).ok().unwrap_or_default()
    }
    pub fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        self.model.embed(texts).ok().unwrap_or_default()
    }
}

#[cfg(feature = "semantic")]
pub fn embed_and_compare(text_a: &str, text_b: &str) -> f64 {
    let Some(idx) = SemanticIndex::global() else {
        return 0.0;
    };
    let a = idx.embed(text_a);
    let b = idx.embed(text_b);
    cosine_f32(&a, &b)
}

#[cfg(feature = "semantic")]
pub fn embed_batch_compare(queries: &[&str], candidates: &[&str]) -> Vec<(usize, usize, f64)> {
    let Some(idx) = SemanticIndex::global() else {
        return vec![];
    };
    let q_vecs = idx.embed_batch(queries);
    let c_vecs = idx.embed_batch(candidates);
    let mut results = Vec::new();
    for (qi, qv) in q_vecs.iter().enumerate() {
        for (ci, cv) in c_vecs.iter().enumerate() {
            let sim = cosine_f32(qv, cv);
            if sim > 0.5 {
                results.push((qi, ci, sim));
            }
        }
    }
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_f32(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.0_f32, 1.0, 0.0];
        let sim = cosine_f32(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_empty() {
        let a: Vec<f32> = vec![];
        let b = vec![1.0_f32];
        assert!(cosine_f32(&a, &b).abs() < f64::EPSILON);
        assert!(cosine_f32(&a, &a).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cosine_zero_vector() {
        let a = vec![0.0_f32, 0.0, 0.0];
        let b = vec![1.0_f32, 2.0, 3.0];
        assert!(cosine_f32(&a, &b).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cosine_opposite() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![-1.0_f32, 0.0];
        let sim = cosine_f32(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[cfg(feature = "semantic")]
    #[test]
    #[ignore]
    fn test_embed_and_compare() {
        let sim = embed_and_compare("price", "amount");
        assert!(sim > 0.7, "price vs amount similarity = {sim}");
    }

    #[cfg(feature = "semantic")]
    #[test]
    #[ignore]
    fn test_embed_batch_compare() {
        let queries = &["price", "name", "email"];
        let candidates = &["cost", "amount", "title", "mail_address"];
        let results = embed_batch_compare(queries, candidates);
        assert!(!results.is_empty());
        let top = &results[0];
        assert!(top.2 > 0.5);
    }
}
