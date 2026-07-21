use std::sync::Mutex;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub(crate) const NEURAL_VECTOR_DIM: usize = 384;

/// Identifies the embedding model (and text-composition format) that
/// produced a cached embedding — bump this if `EmbeddingModel::AllMiniLML6V2`
/// below is ever swapped for a different model, or `compose_text` changes in
/// a way that would change its output, so on-disk caches keyed to the old
/// identity are treated as stale rather than loaded and misused.
pub(crate) const MODEL_VERSION: &str = "all-MiniLM-L6-v2-v1";

pub(crate) struct NeuralEmbedder {
    model: Mutex<TextEmbedding>,
}

impl NeuralEmbedder {
    pub(crate) fn try_new() -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }

    fn compose_text(content: &str, seed_terms: &[String]) -> String {
        let text = if seed_terms.is_empty() {
            content.to_owned()
        } else {
            format!("{content} {}", seed_terms.join(" "))
        };
        text.trim().to_owned()
    }

    pub(crate) fn embed(&self, content: &str, seed_terms: &[String]) -> Vec<f32> {
        let text = Self::compose_text(content, seed_terms);
        if text.is_empty() {
            return vec![0.0; NEURAL_VECTOR_DIM];
        }

        let model = self
            .model
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match model.embed(vec![text], None) {
            Ok(embeddings) if !embeddings.is_empty() => embeddings.into_iter().next().unwrap(),
            Ok(_) => vec![0.0; NEURAL_VECTOR_DIM],
            Err(err) => {
                eprintln!("warning: neural embedding failed: {err}");
                vec![0.0; NEURAL_VECTOR_DIM]
            }
        }
    }

    /// Embeds many (content, seed_terms) pairs in a single batched inference
    /// call — used to precompute a whole corpus at once instead of paying a
    /// separate model-lock/forward-pass round trip per item (the latter is
    /// what made the first neural emoji search of a session take seconds).
    /// Returns `None` on failure rather than zero-vectors: unlike `embed`'s
    /// per-query fallback (which only degrades one search), this result gets
    /// persisted to disk as a trusted cache, so a failure must be visible to
    /// the caller instead of silently written out as "valid" zero-vectors
    /// that would then be loaded and trusted on every future launch.
    pub(crate) fn embed_batch<'a>(
        &self,
        items: impl IntoIterator<Item = (&'a str, &'a [String])>,
    ) -> Option<Vec<Vec<f32>>> {
        let texts: Vec<String> = items
            .into_iter()
            .map(|(content, seed_terms)| Self::compose_text(content, seed_terms))
            .collect();
        if texts.is_empty() {
            return Some(Vec::new());
        }
        let count = texts.len();

        let model = self
            .model
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match model.embed(texts, None) {
            Ok(embeddings) if embeddings.len() == count => Some(embeddings),
            Ok(_) => {
                eprintln!("warning: neural batch embedding returned unexpected count");
                None
            }
            Err(err) => {
                eprintln!("warning: neural batch embedding failed: {err}");
                None
            }
        }
    }

    pub(crate) fn zero_vector() -> Vec<f32> {
        vec![0.0; NEURAL_VECTOR_DIM]
    }
}
