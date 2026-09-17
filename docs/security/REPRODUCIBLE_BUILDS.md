# Reproducible Builds

Strangecoin uses reproducible builds to ensure that release binaries can be independently verified. Every release artifact is accompanied by:

- **SHA256 checksums** for integrity verification
- **cosign signatures** (keyless, via Sigstore/OIDC) for authenticity
- **SLSA Level 3 provenance** for build supply chain integrity

## Verifying a Release

### 1. Download release assets

From the [GitHub Releases page](https://github.com/anomalyco/strangecoin/releases), download:
- The binary for your platform (e.g., `strangecoin-linux-x86_64`)
- `SHA256SUMS.txt` (if published) or individual `.sha256` files
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
  --certificate-identity "https://github.com/anomalyco/strangecoin/.github/workflows/release.yml@refs/tags/v<VERSION>" \
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
  --source-uri github.com/anomalyco/strangecoin \
  --source-tag v<VERSION> \
  strangecoin-linux-x86_64
```

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
