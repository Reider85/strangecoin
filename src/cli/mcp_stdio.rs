//! `strangecoin --mcp-stdio` — MCP-only child-process mode (AI-P02,
//! ADR-0029, spec `docs/spec/mcp_server.md` §4.1).
//!
//! Opens the configured chain database, serves MCP on stdin/stdout and
//! exits on EOF or a `shutdown` request. No P2P, no mining — the IDE
//! plugin spawns this as a trusted local child (process isolation *is* the
//! auth story on stdio, ADR-0029 Alternatives 1/2).
//!
//! Two documented deviations from a literal reading of spec §4.1:
//!
//! - **RocksDB exclusivity** — the normal node must not run against the
//!   same `storage.path` while `--mcp-stdio` holds it (the engine takes an
//!   exclusive process lock). In-process HTTP+SSE (`[mcp] enabled`) is the
//!   co-resident option.
//! - **`--mcp-data-dir <path>`** — overrides both `storage.path` and
//!   `data_dir` (keystore); exists for testability (the `mcp-cli`
//!   round-trip spawns the real binary against a temp chain).
//!
//! Tracing is initialized to **stderr** by `run_async` when this flag is
//! present — stdout belongs to the JSON-RPC stream.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::error::StrangecoinError;
use crate::BlockchainFacade;

/// `--mcp-data-dir <path>` extraction (manual parsing, `--print-genesis-hash`
/// precedent — no clap in the workspace).
pub fn data_dir_override(args: &[String]) -> Result<Option<PathBuf>, StrangecoinError> {
    let Some(index) = args.iter().position(|arg| arg == "--mcp-data-dir") else {
        return Ok(None);
    };
    let value = args.get(index + 1).ok_or_else(|| {
        StrangecoinError::ConfigError("--mcp-data-dir requires a path value".into())
    })?;
    if value.starts_with("--") {
        return Err(StrangecoinError::ConfigError(format!(
            "--mcp-data-dir requires a path value, got `{value}`"
        )));
    }
    Ok(Some(PathBuf::from(value)))
}

pub async fn run(args: &[String], config: &Config) -> Result<(), StrangecoinError> {
    let mut storage = config.storage.clone();
    let mut data_dir = config.data_dir.clone();
    if let Some(overridden) = data_dir_override(args)? {
        storage.path = overridden.clone();
        data_dir = overridden;
    }
    let storage_path = storage.path.clone();

    // Same open path as the node (`Config` → port/chain_id/storage + the
    // three open-path policy knobs), minus every runtime service.
    let port = config.network.listen_addr.port();
    let facade = BlockchainFacade::new(port, config.network_id, storage);
    facade.set_allow_grant_blocks(config.allow_grant_blocks);
    facade.set_allow_zero_state_root(config.zero_state_root_allowed());
    facade.set_pruning(config.pruning_config());

    let identity = crate::wallet::load_or_create_node_keypair(&data_dir)?;
    let backend: Arc<dyn strangecoin_mcp::McpBackend> = Arc::new(facade);
    let server = strangecoin_mcp::McpServer::new(backend, identity);

    tracing::info!(
        target: "mcp",
        storage = %storage_path.display(),
        "serving MCP over stdio"
    );
    strangecoin_mcp::stdio::serve_stdio(server, tokio::io::stdin(), tokio::io::stdout())
        .await
        .map_err(|err| StrangecoinError::Mcp(err.to_string()))
}
