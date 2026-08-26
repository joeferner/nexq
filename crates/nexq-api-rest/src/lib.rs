//! Native REST facade (axum). Complete API surface: full SQS/SNS parity plus the
//! extensions. Route and type definitions here are the source of the OpenAPI spec.
//!
//! **Under construction.** The listener, its authentication, and its error envelope are
//! real; the operation surface is one receive, which is what proves this facade and the
//! SQS one run over a single [`nexq_core::engine::Engine`]. See `todo.md` M9 for the order
//! the rest arrives in — the spec generation via `aide` included, which is why the wire
//! types here stay plain `serde` for now.
//!
//! The facade owns its own listener — see [`Server`] — so `nexq-server` only decides
//! whether to run it.

pub mod auth;
pub mod error;
pub mod messages;
pub mod server;

pub use error::ApiError;
pub use server::Server;
