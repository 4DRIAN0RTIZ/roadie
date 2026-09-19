//! Library half of `roadie`, so integration tests can reach the backend
//! and the TUI state machine; the binary in `main.rs` only wires them up.
pub mod backend;
pub mod cache;
pub mod domain;
pub mod tui;
