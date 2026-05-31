use tracing_subscriber::{EnvFilter, fmt};

fn main() {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("rag-mcp Phase 5 — not yet implemented");
    // Phase 5: build rag-core components from config, wire rmcp server, run stdio/HTTP transport.
}
