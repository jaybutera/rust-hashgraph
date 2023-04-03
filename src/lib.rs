pub mod algorithm;
mod common;

// Some peer identity type? probably make generic
pub type PeerId = u64;

// In milliseconds, I guess. Should work for 500+
// million years.
pub type Timestamp = u128;
