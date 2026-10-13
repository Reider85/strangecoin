# PGP_SETUP.md — Offline PGP Key Generation Procedure (Ops Gate)

**Version:** 1.0
**Date:** 2026-10-13
**Status:** Active — procedure for the open ops gate tracked as **BUG-S1-013**
**Blocks:** mainnet launch / mainnet freeze; encrypted security-report channel

---

## 1. Why this is an ops gate

`SECURITY.md §2` promises a maintainer PGP public key for encrypted security
reporting. As of BUG-S1-013 (2026-10-13) no real key exists in the repository.
Key generation requires:

- an **offline machine** (the private key must never touch a networked dev
  workstation), and
- a **human maintainer** (key material and passphrase are human secrets).

This document is the authoritative procedure. When it is executed, the
checklist in §7 closes BUG-S1-013 and the obligation in
`docs/stage1/STAGE1_SUMMARY.md §6.8`.

## 2. Preconditions

- Offline machine (no network interfaces up, or air-gapped), with GnuPG
  installed (`gpg --version` → 2.2+).
- Encrypted offline storage ready for the private-key backup (external drive
  or encrypted container).
- This repository checked out (or a fresh clone) on a machine used only for
  the export step.

**Never** run the generation steps below on a networked development
workstation. **Never** commit the private key, its backup, or the passphrase
to any repository.

## 3. Generate the key (offline machine)

```bash
gpg --full-generate-key
```

Recommended parameters:

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Key type | RSA 4096 (or ed25519 + cv25519) | Broad client compatibility (RSA) / modern default |
| Key size | 4096 (RSA) | Long-term security margin |
| Expiration | no expiration | Key rotation is not yet operational (see SECURITY.md §7) |
| Name | `Strangecoin Security` | Project identity |
| Email | `security@strangecoin.io` | The channel published in SECURITY.md / BOUNTY.md |

Record the fingerprint shown at the end of generation:

```bash
gpg --fingerprint "Strangecoin Security"
```

## 4. Back up the private key (offline storage)

```bash
# Armored private key — transfer to encrypted offline storage ONLY
gpg --export-secret-keys --armor "Strangecoin Security" > /media/encrypted/strangecoin-security-private.asc
```

Verify the backup can be re-imported on a scratch machine before deleting
anything. The passphrase for the backup must be stored separately from the
backup itself.

## 5. Export and commit the public key

On the repository machine (public key only crosses from the offline machine,
e.g. via a QR code, USB stick, or printed text):

```bash
gpg --export --armor "Strangecoin Security" > docs/security/pgp_key.asc
git add docs/security/pgp_key.asc
```

Then update `SECURITY.md §2`:

- Replace `[TBD — ...]` with the real fingerprint.
- Replace `[pending — file not yet committed]` with a pointer to the committed
  `pgp_key.asc`.
- Update the contacts table row for the PGP key.

## 6. Publish

1. **Keyservers** (public key only):

   ```bash
   gpg --send-keys --keyserver hkps://keys.openpgp.org <KEYID>
   gpg --send-keys --keyserver hkps://keyserver.ubuntu.com <KEYID>
   ```

2. **Bug bounty dashboard** (Immunefi — human action from
   `docs/security/BOUNTY.md §5.1`): upload the same public key to the project
   settings so researchers can encrypt reports end-to-end.

3. **Verification** (any machine):

   ```bash
   gpg --import docs/security/pgp_key.asc
   gpg --fingerprint "Strangecoin Security"
   # fingerprint must match SECURITY.md §2
   gpg --verify <(echo test | gpg --encrypt --armor -r security@strangecoin.io | gpg --decrypt) -
   ```

## 7. Closure checklist (BUG-S1-013 / STAGE1_SUMMARY §6.8)

- [ ] Key generated on offline machine (§3)
- [ ] Private-key backup verified on encrypted offline storage (§4)
- [ ] `docs/security/pgp_key.asc` committed (public key only) (§5)
- [ ] `SECURITY.md §2` fingerprint filled; contacts table updated (§5)
- [ ] Public key uploaded to keys.openpgp.org and keyserver.ubuntu.com (§6.1)
- [ ] Public key uploaded to the Immunefi dashboard (§6.2)
- [ ] Import/verify smoke test passed (§6.3)
- [ ] `analytics/bugfixes-stage1.md` BUG-S1-013 → `fixed` with `Решение` section
- [ ] `docs/stage1/STAGE1_SUMMARY.md §6.8` → closed

## 8. Explicit non-actions

- Do **not** generate the key on the CI runner or any GitHub Actions
  environment — build infrastructure is untrusted for key custody.
- Do **not** paste the fingerprint-only placeholder back into a
  `-----BEGIN PGP PUBLIC KEY BLOCK-----` wrapper; armor blocks are only valid
  around real key material.
- Do **not** treat the security email as confidential until this procedure is
  complete (SECURITY.md §2 states this explicitly).
