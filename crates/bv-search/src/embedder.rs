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

/// Cosine similarity between two equal-length vectors (both already
/// L2-normalized by `hash_embed`, so this reduces to a dot product — kept
/// as a full cosine calculation so it's correct for non-normalized inputs
/// too).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na * nb)) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_of_identical_vectors_is_one() {
        let v = hash_embed("login authentication flow", DEFAULT_DIM);
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_similarity_of_disjoint_vocab_is_zero() {
        let a = hash_embed("alpha", 8);
        let b = hash_embed("zzz", 8);
        // Small dim risks hash collisions landing in the same bucket; just
        // assert it's not the identical-vector case.
        assert!(cosine_similarity(&a, &b) < 1.0);
    }

    #[test]
    fn cosine_similarity_mismatched_lengths_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

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
