use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use async_trait::async_trait;

use crate::error::RagError;
use crate::traits::Embedder;

/// Deterministic hash-based embedder for unit tests and local dev.
///
/// Vectors are not semantically meaningful but are stable: the same text always
/// produces the same vector, enabling reproducible test assertions about the
/// indexing/retrieval pipeline without a real embedding model.
pub struct MockEmbedder {
    pub dimension: usize,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self { dimension: 384 }
    }
}

impl MockEmbedder {
    fn hash_to_vec(&self, text: &str) -> Vec<f32> {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let seed = h.finish();

        (0..self.dimension)
            .map(|i| {
                let mut h2 = DefaultHasher::new();
                (seed ^ (i as u64).wrapping_mul(6364136223846793005)).hash(&mut h2);
                let raw = h2.finish() as f32 / u64::MAX as f32;
                raw * 2.0 - 1.0
            })
            .collect()
    }

    fn normalize(v: &mut Vec<f32>) {
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    fn name(&self) -> &str {
        "mock"
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RagError> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = self.hash_to_vec(t);
                Self::normalize(&mut v);
                v
            })
            .collect())
    }
}
