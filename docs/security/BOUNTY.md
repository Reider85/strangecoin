# BOUNTY.md — Strangecoin Bug Bounty Program

**Версия:** 1.0
**Дата:** 2026-09-17
**Статус:** Active (local preparation; Immunefi integration pending human setup)

---

## 1. Overview

Strangecoin operates a bug bounty program to incentivize responsible disclosure of security vulnerabilities. This program is active from day-1 (Stage 0) and will be integrated with Immunefi upon mainnet readiness.

**Goal:** Identify and remediate security vulnerabilities before they can be exploited, protecting user funds and network integrity.

## 2. Scope

### 2.1 In Scope

The following components and vulnerability types are eligible for bounty rewards:

| Category | Examples |
|----------|----------|
| **Consensus bugs** | Chain reorganization beyond allowed depth, invalid block acceptance, difficulty manipulation, emission overflow, nonce replay |
| **Fund loss** | Theft of funds without private key, unauthorized spending, locked funds permanent loss |
| **Cryptographic weaknesses** | Signature forgery, hash collision exploitation, key recovery from public data, randomness manipulation |
| **Remote Code Execution** | Any RCE vector via P2P, config parsing, serialization, or wallet operations |
| **Denial of Service** | OOM via malformed messages (beyond mitigated vectors in P12), infinite loop DoS, resource exhaustion |
| **Privacy leaks** | Transaction graph deanonymization beyond expected, private key material exposure via side channels |
| **Supply attacks** | Inflation beyond emission schedule, unauthorized coinbase, double-spend via consensus bug |
| **Network attacks** | Eclipse attack (full peer isolation), partition attack, Sybil beyond rate-limiting |

### 2.2 Out of Scope

- **Social engineering:** Phishing, pretexting, or any attack requiring user interaction
- **Low-info reports:** Vulnerabilities without proof-of-concept or clear impact assessment
- **UI-only bugs:** Visual glitches, UX issues without security impact
- **Previously known issues:** Vulnerabilities already disclosed or mitigated in current codebase
- **Theoretical attacks:** Attacks requiring quantum computers or break of secp256k1/SHA-256/blake3
- **Third-party dependencies:** Vulnerabilities in upstream crates (report to upstream first)

## 3. Reward Tiers

Rewards are denominated in SC (Strangecoin) or USD equivalent at time of payout.

| Severity | Description | Reward |
|----------|-------------|--------|
| **Critical** | Direct fund theft, chain split, consensus failure affecting all nodes, RCE | **$100,000** (or 100,000 SC) |
| **High** | Fund loss under specific conditions, significant DoS against network, economic attack | **$10,000** (or 10,000 SC) |
| **Medium** | Limited impact vulnerability, edge-case consensus bug, partial information disclosure | **$1,000** (or 1,000 SC) |
| **Low** | Minor issues with limited security impact, defense-in-depth improvements | **$500** (or 500 SC) |

### 3.1 Reward Multipliers

- **First reporter:** 100% of base reward
- **Critical fix assistance:** Up to 120% if researcher helps develop fix
- **Quality writeup:** Up to 110% for exceptional documentation
- **Duplicate reports:** First valid report receives reward; subsequent duplicates receive 10% courtesy bounty

## 4. Disclosure Policy

### 4.1 Coordinated Disclosure

We follow the standard coordinated disclosure model:

1. **Report submitted:** Researcher contacts security team via encrypted channel
2. **Acknowledgment:** Within **48 hours** (SLA)
3. **Triage:** Initial assessment within **5 business days**
4. **Fix development:** Target fix within **30 days** for critical/high, **90 days** for medium/low
5. **Public disclosure:** After fix is deployed, coordinated disclosure within **14 days**
6. **Total disclosure window:** **90 days** from initial report (regardless of fix status)

### 4.2 Safe Harbor

We commit to:

- **No legal action** against researchers acting in good faith under this program
- **No retaliation** against researchers or their organizations
- **Good faith exemption:** Unintentional violations during research are forgiven if reported promptly
- **Scope adherence:** Researchers must make reasonable effort to stay within scope

### 4.3 Reporting Requirements

Reports must include:

1. **Vulnerability description:** Clear explanation of the issue
2. **Reproduction steps:** Minimal steps to reproduce (PoC code preferred)
3. **Impact assessment:** Potential impact (fund loss, DoS, etc.)
4. **Affected component:** File, module, or subsystem
5. **Suggested fix** (optional but appreciated)

## 5. Immunefi Integration

### 5.1 Current Status (Stage 0)

This bounty program is prepared locally. Full Immunefi integration requires:

1. **Human action:** Create Immunefi project at https://immunefi.com
2. **Human action:** Configure reward tiers and scope on Immunefi dashboard
3. **Human action:** Generate and upload PGP key (see `SECURITY.md`)
4. **Human action:** Set up dedicated security email (security@strangecoin.io or similar)

### 5.2 Planned Integration

Upon mainnet launch (or earlier if program grows):

- Immunefi-managed disclosure workflow
- Automated triage and response
- Community leaderboard
- Reputation tracking for researchers

## 6. Exclusions and Limitations

- Rewards are discretionary and may be adjusted based on impact
- Strangecoin reserves the right to close or modify this program at any time
- Researchers must not exploit vulnerabilities for personal gain beyond the bounty
- Researchers must not access other users' data or funds during testing
- Testing must not disrupt the live network (use regtest/testnet environments)

## 7. Contact

- **Security email:** security@strangecoin.io (PGP key in `SECURITY.md`)
- **Encrypted channel:** PGP-encrypted email preferred
- **Response SLA:** 48 hours for acknowledgment, 5 business days for triage

---

**Note:** This document is a living document and will be updated as the project matures. The Immunefi integration will be completed by the project maintainer as a human action step.
