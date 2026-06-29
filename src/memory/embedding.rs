use std::sync::OnceLock;

use model2vec_rs::model::StaticModel;
use thiserror::Error;

pub(crate) const DIMENSIONS: usize = 256;
pub(crate) const MODEL_ID: &str = "minishlab/potion-base-8M@bf8b056";

static MODEL: OnceLock<StaticModel> = OnceLock::new();

#[derive(Debug, Error)]
pub(crate) enum EmbeddingError {
    #[error("failed to load bundled memory model: {0}")]
    Load(String),
    #[error("memory model returned {actual} dimensions, expected {expected}")]
    Dimensions { actual: usize, expected: usize },
}

pub(crate) fn embed(text: &str) -> Result<Vec<f32>, EmbeddingError> {
    let model = model()?;
    let embedding = model.encode_single(text);
    if embedding.len() != DIMENSIONS {
        return Err(EmbeddingError::Dimensions {
            actual: embedding.len(),
            expected: DIMENSIONS,
        });
    }
    Ok(embedding)
}

pub(crate) fn embed_many(texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let embeddings = model()?.encode(texts);
    if let Some(embedding) = embeddings
        .iter()
        .find(|embedding| embedding.len() != DIMENSIONS)
    {
        return Err(EmbeddingError::Dimensions {
            actual: embedding.len(),
            expected: DIMENSIONS,
        });
    }
    Ok(embeddings)
}

pub(crate) fn as_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn model() -> Result<&'static StaticModel, EmbeddingError> {
    if let Some(model) = MODEL.get() {
        return Ok(model);
    }
    let loaded = StaticModel::from_bytes(
        include_bytes!("../../assets/models/potion-base-8M/tokenizer.json"),
        include_bytes!("../../assets/models/potion-base-8M/model.safetensors"),
        include_bytes!("../../assets/models/potion-base-8M/config.json"),
        None,
    )
    .map_err(|error| EmbeddingError::Load(error.to_string()))?;
    let _ = MODEL.set(loaded);
    MODEL
        .get()
        .ok_or_else(|| EmbeddingError::Load("model initialization failed".to_string()))
}

#[cfg(test)]
mod tests {
    use super::embed;

    #[test]
    fn bundled_model_produces_expected_dimensions() {
        // Given / When
        let embedding = embed("User prefers concise Rust examples.").expect("embedding");

        // Then
        assert_eq!(embedding.len(), 256);
        assert!(embedding.iter().all(|value| value.is_finite()));
    }

    #[test]
    #[ignore = "manual benchmark"]
    fn benchmark_warmed_embedding() {
        let _ = embed("warm up").expect("warm embedding");
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            let _ = embed("User prefers concise Rust examples.").expect("embedding");
        }
        eprintln!("embedding average: {:?}", started.elapsed() / 1_000);
    }
}
