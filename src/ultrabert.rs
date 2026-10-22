//! UltraBERT-Embed-150M forward pass on candle (CPU): token embeddings ->
//! 24 x [RMSNorm -> RoPE attention (sliding window ±128 on layers where
//! config.attention_windows[i] is set) -> residual -> RMSNorm -> SwiGLU MLP ->
//! residual] -> final RMSNorm. Pooling (CLS = position 0) and normalisation
//! happen in embed.rs.
