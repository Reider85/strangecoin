# SECURITY.md — Strangecoin Security Policy

**Версия:** 1.0
**Дата:** 2026-09-17
**Статус:** Active (Stage 0)

---

## 1. Security Contacts

| Channel | Details |
|---------|---------|
| **Security Email** | security@strangecoin.io |
| **PGP Key** | `docs/security/pgp_key.asc` (see below) |
| **Bug Bounty** | See `BOUNTY.md` for rewards and scope |
| **Incident Response** | See `INCIDENT_RESPONSE.md` for triage and response plan |

## 2. PGP Key

A PGP key is provided for encrypted communication. **Important:** The key below is a placeholder. The project maintainer must generate a real PGP key and replace this placeholder before mainnet launch.

```
-----BEGIN PGP PUBLIC KEY BLOCK-----

[PLACEHOLDER: Maintainer must generate real PGP key]
[Replace this block with actual key generated via:]
[gpg --gen-key]
[gpg --export --armor security@strangecoin.io > docs/security/pgp_key.asc]

Key fingerprint: [TO BE FILLED BY MAINTAINER]

-----END PGP PUBLIC KEY BLOCK-----
```

### 2.1 Key Generation Instructions

For the project maintainer:

```bash
# Generate key (interactive)
gpg --full-generate-key
# Select: RSA 4096, no expiration, security@strangecoin.io

# Export public key
gpg --export --armor security@strangecoin.io > docs/security/pgp_key.asc

# Verify
gpg --fingerprint security@strangecoin.io
```

## 3. Response SLA

| Action | SLA |
|--------|-----|
| Acknowledgment of report | 48 hours |
| Initial triage | 5 business days |
| Critical/High fix target | 30 days |
| Medium/Low fix target | 90 days |
| Public disclosure after fix | 14 days |
| **Total disclosure window** | **90 days** |

## 4. Scope Summary

**In scope:** Consensus bugs, fund loss, cryptographic weaknesses, RCE, DoS, privacy leaks, supply attacks, network attacks.

**Out of scope:** Social engineering, low-info reports, UI-only bugs, third-party dependency issues, theoretical quantum attacks.

Full scope details in `BOUNTY.md`.

## 5. Safe Harbor

We commit to:

- No legal action against good-faith security research
- No retaliation against researchers
- Good faith exemption for unintentional scope violations
- 90-day coordinated disclosure window

## 6. Security Best Practices for Users

### 6.1 Wallet Security

- Store keystore files (`keystore/*.json`) in encrypted filesystem
- Use strong passwords (12+ characters, mixed case, numbers, symbols)
- Never share private keys or keystore files
- Backup keystore to offline storage

### 6.2 Node Security

- Keep software updated to latest version
- Use firewall to restrict inbound connections
- Monitor logs for suspicious activity
- Run node on dedicated machine if handling significant value

### 6.3 Network Security

- **Stage 0:** P2P traffic is plaintext TCP (no encryption)
  - Mitigation: Run nodes on trusted networks or use VPN
  - Noise Protocol Framework planned for Stage 2
- **Stage 2+:** Noise Protocol encryption for all P2P traffic

## 7. Known Security Considerations

| Risk | Status | Mitigation |
|------|--------|------------|
| P2P plaintext (Stage 0) | Known | VPN/trusted network; Noise Protocol planned Stage 2 |
| No key rotation | Known | Keystore uses PBKDF2+AES-GCM; key rotation deferred to Stage 2 |
| LevelDB LOCK contention | Mitigated | Retry logic with exponential backoff (P15) |
| Future timestamp attack | Mitigated | MTP + MAX_FUTURE_TIME validation (P09) |
| 51% attack (PoW phase) | Accepted | Monitor hashrate concentration; PoS migration (Stage 7) |
| MEV frontrunning | Deferred | Threshold encryption mempool planned Stage 5 |

## 8. Security Audit

- **Stage 0:** Internal review only (self-audit against ARCHITECT3.md §17 checklist)
- **Stage 1:** External security audit recommended before mainnet
- **Stage 5+:** Formal verification of consensus (TLA+ already in progress from P23)

## 9. Incident Response

For active security incidents, see `INCIDENT_RESPONSE.md`.

**Emergency contact:** security@strangecoin.io (PGP encrypted)

**Critical vulnerability:** Skip standard disclosure — contact immediately via PGP email.

---

**Note:** This document will be updated as the project matures. PGP key must be replaced by maintainer before mainnet launch.
