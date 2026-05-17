## causes_crypto

Single source of truth for the workspace's rustls `CryptoProvider`.

The workspace selects `reqwest`'s `rustls-no-provider` feature so that every rustls user (`octocrab`, `sqlx`, `tonic`, `rustls-acme`, the AWS SDK via `aws-smithy-http-client`) shares one provider with no dual-provider conflicts.
In exchange, each binary that drives rustls must install a `CryptoProvider` exactly once at startup.

### Usage

```rust
fn main() {
    causes_crypto::install_default_provider();
    // ...
}
```

In tests that build a `reqwest::Client` (or any other rustls-backed client) call the same function as the first line of the test.
The call is `Once`-guarded and idempotent.

The provider is `ring`; change it here if you need to swap the workspace.
