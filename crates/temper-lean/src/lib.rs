//! Lean specification checking and native app cores (ADR-0182).
//!
//! Candidate generation is an external concern. This crate owns the immutable
//! contract, checked implementation boundary, benchmark selection, and review
//! required before execution by a Rust service.

pub mod artifact;
mod buffers;
mod call_scope;
pub mod cli;
mod ffi_build;
pub mod manifest;
pub mod native;
mod process;
pub mod search;
mod source;
pub mod toolchain;

use sha2::{Digest, Sha256};

/// Content identity with length framing, so adjacent inputs cannot alias.
pub fn identity(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}
