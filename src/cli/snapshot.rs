//! `sc snapshot export` / `sc snapshot import` (S3-P13, ADR-0026).
//!
//! File-based snapshot transport for Stage 3 — no `net` feature required
//! (storage tooling, the `--print-genesis-hash` precedent for ungated
//! dispatch). P2P `GET_SNAPSHOT` is a Stage 4+ residual.
//!
//! Parsing follows the `shadow replay` style: manual `--key=value` flags,
//! no clap in the workspace. Auto-generation cadence is **not** wired into
//! the node (deviation: ops cron / `--every N` guard on export — the ADR
//! height-triggered hook would need config surface + archival gating that
//! is not trivial enough to rush into S3-P13).

use std::io::Write;
use std::path::PathBuf;

use strangecoin_storage::{
    export as export_snapshot, import as import_snapshot, meta_key,
    verify_file as verify_snapshot_file, Cf, RocksDbEngine, StorageEngine, META_TIP_HEIGHT,
};

/// Parsed arguments for `snapshot export` / `snapshot import`.
#[derive(Debug, PartialEq, Eq)]
pub struct SnapshotArgs {
    pub action: SnapshotAction,
    /// RocksDB directory of the node database.
    pub data_dir: PathBuf,
    /// Export: output file or directory. Import: input file.
    pub file: PathBuf,
    /// Expected/declared network id (ADR-0026 step b). Default 3 = regtest.
    pub network_id: u32,
    /// Export only: refuse unless `height % every == 0` (height-triggered
    /// cadence guard for scripted generation; default = no guard).
    pub every: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotAction {
    Export,
    Import,
}

/// Parse `--key=value` flags for a snapshot subcommand.
///
/// `action` comes from the subcommand word (`export` / `import`).
pub fn parse_snapshot_args(
    action: SnapshotAction,
    args: &[String],
) -> Result<SnapshotArgs, String> {
    let mut data_dir = PathBuf::from("data/rocksdb");
    let mut file: Option<PathBuf> = None;
    let mut network_id: u32 = 3;
    let mut every: Option<u64> = None;

    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            return Err(format!("ожидался флаг --key=value, получено: {arg}"));
        };
        match key {
            "--data-dir" => data_dir = PathBuf::from(value),
            "--file" | "--out" => file = Some(PathBuf::from(value)),
            "--network-id" => {
                network_id = value
                    .parse()
                    .map_err(|_| format!("--network-id должен быть целым: {value}"))?;
            }
            "--every" => {
                every =
                    Some(value.parse().map_err(|_| {
                        format!("--every должен быть неотрицательным целым: {value}")
                    })?);
            }
            other => return Err(format!("неизвестный флаг: {other}")),
        }
    }

    let file = file.ok_or("--file обязателен")?;
    if every.is_some() && action != SnapshotAction::Export {
        return Err("--every действует только для export".to_string());
    }
    Ok(SnapshotArgs {
        action,
        data_dir,
        file,
        network_id,
        every,
    })
}

/// Run `sc snapshot export|import`. Returns the process exit code
/// (0 = success; Err → caller prints and exits 1).
pub fn snapshot(args: &[String]) -> Result<i32, String> {
    let Some(sub) = args.first().map(String::as_str) else {
        return Err("ожидалась подкоманда: export | import".to_string());
    };
    let action = match sub {
        "export" => SnapshotAction::Export,
        "import" => SnapshotAction::Import,
        other => return Err(format!("неизвестная подкоманда snapshot: {other}")),
    };
    let parsed = parse_snapshot_args(action, &args[1..])?;

    let engine = RocksDbEngine::open(&parsed.data_dir)
        .map_err(|e| format!("не удалось открыть БД {}: {e}", parsed.data_dir.display()))?;
    let dyn_engine: &dyn StorageEngine = &engine;

    match parsed.action {
        SnapshotAction::Export => {
            // Height cadence guard BEFORE writing (height-triggered, no clock).
            if let Some(every) = parsed.every {
                let tip = dyn_engine
                    .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
                    .map_err(|e| e.to_string())?
                    .ok_or("нет tip_height — экспортировать нечего")?;
                let tip = u64::from_be_bytes(
                    tip.as_slice()
                        .try_into()
                        .map_err(|_| "tip_height wrong length".to_string())?,
                );
                if every == 0 || tip % every != 0 {
                    return Err(format!(
                        "height {tip} не кратен --every {every} (height-triggered cadence)"
                    ));
                }
            }
            let header = export_snapshot(dyn_engine, &parsed.file, parsed.network_id)
                .map_err(|e| format!("export failed: {e}"))?;
            // Post-write verify: the file we just produced must pass the
            // same gate an importer runs (belt and suspenders).
            verify_snapshot_file(&parsed.file, parsed.network_id)
                .map_err(|e| format!("post-export verify failed: {e}"))?;
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "exported height={} state_root={}",
                header.height,
                hex::encode(header.state_root)
            );
            Ok(0)
        }
        SnapshotAction::Import => {
            let header = import_snapshot(dyn_engine, &parsed.file, parsed.network_id)
                .map_err(|e| format!("import failed: {e}"))?;
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "imported height={} network_id={}",
                header.height, header.network_id
            );
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_requires_file() {
        let err = parse_snapshot_args(SnapshotAction::Export, &flags(&["--data-dir=x"]))
            .expect_err("file required");
        assert!(err.contains("--file"));
    }

    #[test]
    fn parse_reads_all_flags() {
        let parsed = parse_snapshot_args(
            SnapshotAction::Export,
            &flags(&[
                "--data-dir=/tmp/db",
                "--out=/tmp/s.scsnap",
                "--network-id=1",
                "--every=1000",
            ]),
        )
        .expect("parse");
        assert_eq!(parsed.action, SnapshotAction::Export);
        assert_eq!(parsed.data_dir, PathBuf::from("/tmp/db"));
        assert_eq!(parsed.file, PathBuf::from("/tmp/s.scsnap"));
        assert_eq!(parsed.network_id, 1);
        assert_eq!(parsed.every, Some(1000));
    }

    #[test]
    fn parse_defaults_to_regtest_and_no_guard() {
        let parsed = parse_snapshot_args(SnapshotAction::Import, &flags(&["--file=s.scsnap"]))
            .expect("parse");
        assert_eq!(parsed.network_id, 3, "regtest default");
        assert_eq!(parsed.every, None);
        assert_eq!(parsed.data_dir, PathBuf::from("data/rocksdb"));
    }

    #[test]
    fn parse_rejects_unknown_and_malformed() {
        assert!(
            parse_snapshot_args(SnapshotAction::Export, &flags(&["--file=f", "--bogus=1"]))
                .expect_err("unknown")
                .contains("--bogus")
        );
        assert!(
            parse_snapshot_args(SnapshotAction::Export, &flags(&["--file=f", "positional"]))
                .is_err()
        );
        assert!(
            parse_snapshot_args(SnapshotAction::Export, &flags(&["--file=f", "--every=abc"]))
                .is_err()
        );
    }

    #[test]
    fn every_rejected_on_import() {
        let err = parse_snapshot_args(SnapshotAction::Import, &flags(&["--file=f", "--every=10"]))
            .expect_err("every is export-only");
        assert!(err.contains("export"));
    }

    #[test]
    fn subcommand_dispatch_rejects_unknown() {
        let err = snapshot(&["bogus".to_string()]).expect_err("unknown sub");
        assert!(err.contains("bogus"));
        let err = snapshot(&[]).expect_err("missing sub");
        assert!(err.contains("export"));
    }
}
