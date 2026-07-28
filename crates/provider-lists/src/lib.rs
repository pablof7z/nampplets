//! Exact `@napplet/nap` 0.28.0 `lists` provider contract.
//!
//! The domain is *runtime-mediated NIP-51 list mutation*. Rust owns the whole
//! decision: which lists exist, which item types each accepts, whether a
//! requested mutation actually changes anything, and what the resulting entry
//! set is. NMP remains the only canonical owner of the replaceable event, its
//! signature, its relays, and the durable write.
//!
//! A mutation that changes nothing produces a truthful result and *no write*.
//! A mutation that changes something produces an exact write proposal for
//! native approval; the napplet's `.result` envelope is emitted from the
//! receipt, never optimistically.

pub const DOMAIN: &str = "lists";
pub const PINNED_NAP_PROTOCOL: &str = "napplet-web@0.28.0";

mod catalog;
mod provider;
mod session;
mod types;
mod validate;
mod wire;
mod write;

pub use catalog::*;
pub use provider::*;
pub use types::*;
