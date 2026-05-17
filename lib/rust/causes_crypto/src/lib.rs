//! Workspace-wide rustls `CryptoProvider` initialisation.
//!
//! The workspace selects `reqwest`'s `rustls-no-provider` feature so that all rustls users (octocrab, sqlx, tonic, rustls-acme, AWS SDK via aws-smithy-http-client) share a single provider with no dual-provider conflicts.
//! In exchange, every binary that builds a `reqwest::Client` or otherwise drives rustls before something else has installed a provider must install one explicitly at startup.
//! This crate is the single source of truth for the workspace's provider choice (ring) and the idempotent install.

use std::sync::Once;

/// Install rustls's ring `CryptoProvider` as the process-level default.
/// Safe to call from anywhere — the `Once` guard means concurrent or repeat calls are no-ops, and the install attempt is allowed to fail silently if a provider was already installed by some other path.
pub fn install_default_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
