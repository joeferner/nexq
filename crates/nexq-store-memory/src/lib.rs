//! In-memory storage backend.
//!
//! Everything lives in process and nothing survives a restart, so this backend cannot
//! support multi-node HA — a second node could never observe the same state. It is
//! still a first-class choice for single-node deployments, and the default one.
//!
//! Its other job is to be the reference implementation: it has no external
//! dependencies, so it is the backend the `Store` trait is designed against and the
//! one the conformance suite is written for first.
