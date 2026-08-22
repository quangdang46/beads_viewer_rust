//! Hash embedder — port of Go `pkg/search/hash_embedder.go`:
//! FNV-1a 64-bit signed buckets -> f32[dim] -> L2 normalize.

/// Default embedding dimension (Go: DefaultEmbeddingDim).
pub const DEFAULT_DIM: usize = 384;

/// FNV-1a 64-bit hash of a string, mapped to a signed bucket index.
fn fnv1a64_bucket(token: &str, dim: usize) -> usize {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in token.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash % dim as u64) as usize
}

/// Hash-embed a document into f32[dim], L2-normalized.
pub fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];
    // Tokenize by whitespace and punctuation.
    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|t| !t.is_empty())
        .collect();

    for token in &tokens {
        let bucket = fnv1a64_bucket(token, dim);
        vec[bucket] += 1.0;
    }

    // L2 normalize.
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_is_normalized() {
        let v = hash_embed("hello world test", DEFAULT_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn different_texts_different_embeddings() {
        let a = hash_embed("login authentication", DEFAULT_DIM);
        let b = hash_embed("database migration", DEFAULT_DIM);
        assert_ne!(a, b);
    }

    #[test]
    fn same_text_same_embedding() {
        let a = hash_embed("deterministic output", DEFAULT_DIM);
        let b = hash_embed("deterministic output", DEFAULT_DIM);
        assert_eq!(a, b);
    }

    #[test]
    fn default_dim_is_384() {
        assert_eq!(DEFAULT_DIM, 384);
    }
}
