//! CLI-команды P02. Сегодня поддерживается `--print-genesis-hash`
//! (BUG-S0-009: машинно-читаемый stdout) и `shadow replay` (S2-P20,
//! Stage 6 prep — replay mainnet-блоков на staging-цепь с детекцией
//! consensus divergence). Полный CLI-спек ARCHITECT3 §3.10
//! (`--balance`, `--send --sign`, `sc block`, `sc node start`, faucet, …)
//! деферрен вместе с api-слоем до Stage 4 (BUG-S0-020); имя бинаря
//! `sc` — тоже Stage 4 (сейчас `strangecoin shadow replay …`).

use crate::consensus;
use std::io::Write;
use std::path::Path;
use strangecoin_core::serialize;

pub fn print_genesis_hash() {
    // Try current dir first, then exe dir
    let genesis_path = if Path::new("genesis.json").exists() {
        std::path::PathBuf::from("genesis.json")
    } else {
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        exe_dir.join("genesis.json")
    };
    let block =
        consensus::load_genesis(genesis_path.to_str().unwrap()).expect("Failed to load genesis");
    let hash = serialize::block_hash(&block);
    let _ = writeln!(std::io::stdout(), "0x{}", hex::encode(hash));
}

/// `sc shadow replay` (S2-P20): parsed CLI arguments — a
/// [`strangecoin_net::ReplayConfig`] plus the staging-facade opening
/// parameters that are not part of the replay config itself.
#[cfg(feature = "net")]
#[derive(Debug)]
pub struct ShadowReplayArgs {
    pub config: strangecoin_net::ReplayConfig,
    /// LevelDB directory of the local staging chain.
    pub data_dir: std::path::PathBuf,
    /// Consensus rules of the staging chain (default 1 = mainnet rules —
    /// a shadow replay tests mainnet history, so the default must match
    /// mainnet; regtest permissiveness is a deliberate opt-in).
    pub chain_id: u32,
}

/// Parse `--key=value` flags for `shadow replay` (manual parsing, same
/// style as `--print-genesis-hash` — no clap in the workspace). Bare
/// `--stop-on-divergence` toggles early halt; `--blocks-file` is
/// required; `--max-blocks=0` (default) means unlimited.
#[cfg(feature = "net")]
pub fn parse_shadow_replay_args(args: &[String]) -> Result<ShadowReplayArgs, String> {
    let mut staging_node_url = String::new();
    let mut blocks_file: Option<std::path::PathBuf> = None;
    let mut stop_on_divergence = false;
    let mut max_blocks: usize = 0;
    let mut data_dir = std::path::PathBuf::from("data/shadow-staging");
    let mut chain_id: u32 = 1;

    for arg in args {
        if arg == "--stop-on-divergence" {
            stop_on_divergence = true;
            continue;
        }
        let Some((key, value)) = arg.split_once('=') else {
            return Err(format!(
                "ожидался флаг --key=value или --stop-on-divergence, получено: {arg}"
            ));
        };
        match key {
            "--staging-url" => staging_node_url = value.to_string(),
            "--blocks-file" => blocks_file = Some(std::path::PathBuf::from(value)),
            "--max-blocks" => {
                max_blocks = value.parse().map_err(|_| {
                    format!("--max-blocks должен быть неотрицательным целым: {value}")
                })?
            }
            "--data-dir" => data_dir = std::path::PathBuf::from(value),
            "--chain-id" => {
                chain_id = value
                    .parse()
                    .map_err(|_| format!("--chain-id должен быть целым: {value}"))?
            }
            other => return Err(format!("неизвестный флаг: {other}")),
        }
    }

    let blocks_file = blocks_file.ok_or("--blocks-file обязателен")?;
    Ok(ShadowReplayArgs {
        config: strangecoin_net::ReplayConfig {
            staging_node_url,
            mainnet_blocks_file: blocks_file,
            stop_on_divergence,
            max_blocks,
        },
        data_dir,
        chain_id,
    })
}

/// `sc shadow replay` (S2-P20): open a local staging chain, replay the
/// mainnet blocks file through it, print the report and return the exit
/// code (0 = staging agrees with mainnet history, 2 = divergences found).
#[cfg(feature = "net")]
pub async fn shadow_replay(args: &[String]) -> Result<i32, String> {
    let parsed = parse_shadow_replay_args(args)?;
    let staging = crate::BlockchainFacade::open_local(&parsed.data_dir, parsed.chain_id);
    let report = strangecoin_net::replay_blocks(&parsed.config, &staging)
        .await
        .map_err(|err| err.to_string())?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(
        out,
        "shadow replay: {} blocks loaded, {} replayed, {} accepted, {} rejected",
        report.total_loaded, report.replayed, report.accepted, report.rejected
    );
    for div in &report.divergences {
        let _ = writeln!(
            out,
            "DIVERGENCE height={} mainnet={:?} staging={:?} reason={}",
            div.block_height,
            div.mainnet_status,
            div.staging_status,
            div.divergence_reason.as_deref().unwrap_or("-")
        );
    }
    if report.stopped_early {
        let _ = writeln!(out, "остановлено на первом divergence (stop_on_divergence)");
    }
    if report.divergences.is_empty() {
        Ok(0)
    } else {
        Ok(2)
    }
}

#[cfg(all(test, feature = "net"))]
mod tests {
    use super::*;

    fn flags(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_shadow_replay_requires_blocks_file() {
        let err = parse_shadow_replay_args(&flags(&[])).expect_err("blocks-file required");
        assert!(err.contains("--blocks-file"));
    }

    #[test]
    fn parse_shadow_replay_reads_all_flags() {
        let parsed = parse_shadow_replay_args(&flags(&[
            "--staging-url=sc://staging:8081",
            "--blocks-file=mainnet.jsonl",
            "--max-blocks=100",
            "--stop-on-divergence",
            "--data-dir=/tmp/shadow",
            "--chain-id=3",
        ]))
        .expect("parse");
        assert_eq!(parsed.config.staging_node_url, "sc://staging:8081");
        assert_eq!(
            parsed.config.mainnet_blocks_file,
            std::path::PathBuf::from("mainnet.jsonl")
        );
        assert_eq!(parsed.config.max_blocks, 100);
        assert!(parsed.config.stop_on_divergence);
        assert_eq!(parsed.data_dir, std::path::PathBuf::from("/tmp/shadow"));
        assert_eq!(parsed.chain_id, 3);
    }

    #[test]
    fn parse_shadow_replay_defaults() {
        let parsed = parse_shadow_replay_args(&flags(&["--blocks-file=f.json"])).expect("parse");
        assert!(parsed.config.staging_node_url.is_empty());
        assert_eq!(parsed.config.max_blocks, 0);
        assert!(!parsed.config.stop_on_divergence);
        assert_eq!(
            parsed.data_dir,
            std::path::PathBuf::from("data/shadow-staging")
        );
        assert_eq!(parsed.chain_id, 1);
    }

    #[test]
    fn parse_shadow_replay_rejects_unknown_and_malformed() {
        assert!(
            parse_shadow_replay_args(&flags(&["--blocks-file=f", "--bogus=1"]))
                .expect_err("unknown flag")
                .contains("--bogus")
        );
        assert!(parse_shadow_replay_args(&flags(&["--blocks-file=f", "positional"])).is_err());
        assert!(
            parse_shadow_replay_args(&flags(&["--blocks-file=f", "--max-blocks=abc"])).is_err()
        );
    }
}
