# Reproducible Builds

Strangecoin uses reproducible builds to ensure that release binaries can be independently verified. Every release artifact is accompanied by:

- **SHA256 checksums** for integrity verification
- **cosign signatures** (keyless, via Sigstore/OIDC) for authenticity
- **SLSA Level 3 provenance** for build supply chain integrity

## Verifying a Release

### 1. Download release assets

From the [GitHub Releases page](https://github.com/Reider85/strangecoin/releases), download:
- The binary for your platform (e.g., `strangecoin-linux-x86_64`)
- `SHA256SUMS.txt` (published) or individual `.sha256` files
- `.cosign` signature file
- `.cosign.pem` certificate file

### 2. Verify checksum

```bash
# Using individual .sha256 file
shasum -a 256 -c strangecoin-linux-x86_64.sha256

# Or compare manually
shasum -a 256 strangecoin-linux-x86_64
# Output should match the published checksum
```

### 3. Verify cosign signature

```bash
cosign verify-blob \
  --signature strangecoin-linux-x86_64.cosign \
  --certificate strangecoin-linux-x86_64.cosign.pem \
  --certificate-identity "https://github.com/Reider85/strangecoin/.github/workflows/release.yml@refs/tags/v<VERSION>" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  strangecoin-linux-x86_64
```

Expected output: `Verified OK`

### 4. Verify SLSA provenance

```bash
# Install slsa-verifier
go install github.com/slsa-framework/slsa-verifier/v2/cli/slsa-verifier@latest

# Verify provenance
slsa-verifier verify-artifact \
  --provenance-path multiple.intoto.jsonl \
  --source-uri github.com/Reider85/strangecoin \
  --source-tag v<VERSION> \
  strangecoin-linux-x86_64
```

## Verified Release Runs

### v0.0.1-rc1 (2026-10-10) — first pipeline run (BUG-S1-001, invariant #22)

- **Workflow run:** https://github.com/Reider85/strangecoin/actions/runs/38071514479
- **Result:** green on all jobs — 6 matrix builds (linux x86_64/aarch64, macOS x86_64/aarch64, windows x86_64/aarch64) + `aggregate-hashes` + `sign` + `SLSA Provenance` (detect-env/generator/upload-assets/final) + `Create Release`.
- **Release:** https://github.com/Reider85/strangecoin/releases/tag/v0.0.1-rc1 — 31 assets: 6 binaries, 6 `.sha256`, 6 `.cosign`, 6 `.cosign.pem`, `SHA256SUMS.txt`, `multiple.intoto.jsonl` (SLSA L3 provenance).
- **Tag commit:** `a830c51f74ea407ff5e5b2507773928a6f1ac2f2`.

**Published SHA256 (`SHA256SUMS.txt`):**

| Artifact | SHA256 |
|---|---|
| strangecoin-linux-aarch64 | `b3d806decd49a1f6ea79e86681a4776d213e17fa610778f947304dbec8455253` |
| strangecoin-linux-x86_64 | `c2c4eb590c622a374eb6e6bc70fb8bc8ef02f3a3d01d617dd460db395336af8e` |
| strangecoin-macos-aarch64 | `906d447759162f62cf819031aec1af84fcfafeacf356f558cdeaf10a53657b2d` |
| strangecoin-macos-x86_64 | `4a0cc74e15f7cc8abac35416a6717611d175a6bb1dccd56fa191991dff3a10d6` |
| strangecoin-windows-aarch64.exe | `52f549b90617959642d18fb609d029e378f9e7867bf8e386bf0f1d5f11e69599` |
| strangecoin-windows-x86_64.exe | `b392235aecf8a279bb574c23b205d5d06b33220b872724615f8f54606dd8dc97` |

**Independent verification (Windows x86_64 + Linux x86_64 artifacts, 2026-10-10):**

1. Local SHA256 of downloaded artifacts matched `SHA256SUMS.txt`:
   - `strangecoin-windows-x86_64.exe` → MATCH
   - `strangecoin-linux-x86_64` → MATCH
2. `cosign verify-blob` (cosign v3.1.3) against the release workflow OIDC identity:
   ```
   cosign verify-blob --signature strangecoin-windows-x86_64.exe.cosign \
     --certificate strangecoin-windows-x86_64.exe.cosign.pem \
     --certificate-identity "https://github.com/Reider85/strangecoin/.github/workflows/release.yml@refs/tags/v0.0.1-rc1" \
     --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
     strangecoin-windows-x86_64.exe
   ```
   Output: `Verified OK` (exit 0).
3. `slsa-verifier verify-artifact` (slsa-verifier v2.7.1) against the published provenance:
   ```
   slsa-verifier verify-artifact strangecoin-windows-x86_64.exe \
     --provenance-path multiple.intoto.jsonl \
     --source-uri github.com/Reider85/strangecoin \
     --source-tag v0.0.1-rc1
   ```
   Output: `Verified build using builder "https://github.com/slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@refs/tags/v2.1.0" at commit a830c51f74ea407ff5e5b2507773928a6f1ac2f2` — `PASSED` (exit 0).

### v1.5.0-stage1.5 (2026-10-18) — Stage 1.5 gate release (S1.5-FBP11)

- **Workflow run:** https://github.com/Reider85/strangecoin/actions/runs/39110855701
- **Result:** green on all jobs — 13/13: 6 matrix builds (linux x86_64/aarch64, macOS x86_64/aarch64, windows x86_64/aarch64) + `aggregate-hashes` + `sign` + `SLSA Provenance` (detect-env/generator/upload-assets/final) + `Create Release`. Verified via GitHub REST API (`/repos/Reider85/strangecoin/actions/runs/39110855701/jobs`) on 2026-10-20.
- **Release:** https://github.com/Reider85/strangecoin/releases/tag/v1.5.0-stage1.5 — 26 assets: 6 binaries, 6 `.sha256`, 6 `.cosign`, 6 `.cosign.pem`, `SHA256SUMS.txt`, `multiple.intoto.jsonl` (SLSA L3 provenance). Published 2026-10-18T08:19:51Z.
- **Tag commit:** `1f133c7e1910ac870050d6fc0da0b37d3b5338df` (annotated tag, tagger Reider85; verified pushed via `git ls-remote --tags origin`).

**Published SHA256 (`SHA256SUMS.txt`):**

| Artifact | SHA256 |
|---|---|
| strangecoin-linux-aarch64 | `50cbe4ce7ec15a0bcdf4fc00e8993f71ccd41554abd884e10f797aa8ad3ff54a` |
| strangecoin-linux-x86_64 | `01ed5b88c7da29e680bc4ac3784149abc914cb6428a5b1bc9820834fa93a857b` |
| strangecoin-macos-aarch64 | `1f1cf898aaecfb97d6b2f0c5da08bd22ee430b80b801dfc5805e03409e9e5cd7` |
| strangecoin-macos-x86_64 | `c5d396f38c60dd7704e22ade95a0055bf549ba416186af25e3f225e52c7f1b53` |
| strangecoin-windows-aarch64.exe | `5fd480d082688fcb7444ba49f2f29ee13478abc464e773d1bbc7fe3616cddd4e` |
| strangecoin-windows-x86_64.exe | `dd7530c5295771dd1284361d4a92c1dd58ec74a871cab2eef8661f5539c09ea7` |

**Independent verification status (2026-10-20):**

1. Workflow-run status + job conclusions + published asset inventory + `SHA256SUMS.txt` contents — **verified** via unauthenticated GitHub REST API (`gh` CLI not installed on this Windows host; REST API used instead).
2. `cosign verify-blob` + `slsa-verifier verify-artifact` — **not verified locally + reason**: `cosign`, `slsa-verifier`, and `gh` are not installed on the Windows verification host. Pipeline-level cosign/SLSA jobs were green in the same run. Reproduce independently with the commands in §«Verifying a Release» above, substituting `v<VERSION>` = `v1.5.0-stage1.5` (certificate identity: `https://github.com/Reider85/strangecoin/.github/workflows/release.yml@refs/tags/v1.5.0-stage1.5`).

## Building Locally (Reproducibility Check)

To verify that a locally built binary matches the release:

### Prerequisites

- Rust stable toolchain (same version as CI — check `rust-toolchain.toml` or CI config)
- `RUSTFLAGS` must include `--remap-path-prefix`

### Build

```bash
# Clone at the release tag
git checkout v<VERSION>

# Build with the same flags as CI
RUSTFLAGS="--remap-path-prefix $(pwd)=." cargo build --release --target x86_64-unknown-linux-gnu

# Compare checksums
shasum -a 256 target/x86_64-unknown-linux-gnu/release/strangecoin
# Must match the published checksum
```

### Build flags (determinism)

The following flags ensure reproducible builds:

| Flag | Purpose |
|------|---------|
| `RUSTFLAGS="--remap-path-prefix $PWD=."` | Strips absolute build paths from binary |
| `lto = true` | Link-Time Optimization for deterministic codegen |
| `codegen-units = 1` | Single codegen unit prevents non-deterministic partitioning |
| `strip = true` | Strips debug symbols for smaller, more stable binaries |

These are set in `Cargo.toml` (`[profile.release]`) and in `.github/workflows/release.yml`.

## Known Limitations

1. **Rust compiler version**: Different Rust versions may produce different binaries. Always use the same stable version as the CI pipeline.

2. **OS-specific differences**: Binaries built on Linux vs macOS vs Windows are inherently different. Cross-compiled binaries (e.g., aarch64 on x86_64) may also differ from natively compiled ones.

3. **Cargo.lock**: Reproducibility requires an identical `Cargo.lock`. The lockfile is committed to the repository — always build from a tagged release commit.

4. **Time dependencies**: Some build scripts may embed timestamps. The `--remap-path-prefix` flag mitigates path-based differences, but other time sources are not stripped by default.

## SLSA Provenance

Each release generates a [SLSA Level 3](https://slsa.dev/) provenance attestation via the `slsa-framework/slsa-github-generator`. This attestation:

- Cryptographically binds the build to the source commit
- Records the build environment (runner OS, toolchain version)
- Is signed by the GitHub Actions OIDC identity (keyless)
- Can be verified independently using `slsa-verifier`

## References

- [SLSA — Supply-chain Levels for Software Artifacts](https://slsa.dev/)
- [cosign — Software Signing](https://docs.sigstore.dev/cosign/overview/)
- [Sigstore — Keyless Signing](https://www.sigstore.dev/)
- [Reproducible Builds](https://reproducible-builds.org/)
