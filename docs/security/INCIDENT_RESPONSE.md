# INCIDENT_RESPONSE.md — Strangecoin Incident Response Plan

**Версия:** 2.0 (D02)
**Дата:** 2026-09-28
**Статус:** Stage 0 — выполнен в D02 (debt prompt)
**Связанные документы:** `THREAT_MODEL.md`, `ARCHITECT3.md`

---

## 1. Scope

Настоящий документ описывает procedures для реагирования на security incidents в Strangecoin network. Охватывает:

- Consensus bugs (chain split, inflation, double-spend)
- Network attacks (eclipse, partition, DoS)
- Key compromise (wallet, keystore)
- Build compromise (malicious binary)
- Smart contract vulnerabilities (Stage 1.5+)

---

## 2. Severity Levels

| Level | Description | Examples | Response Time |
|-------|-------------|----------|---------------|
| **Critical** | Active exploit, funds at risk, chain halted | 51% attack, inflation bug, consensus split | **4 hours** |
| **High** | Vulnerability confirmed, no active exploit yet | Key recovery, DoS vector, build compromise | **24 hours** |
| **Medium** | Potential vulnerability, needs investigation | Unusual network behavior, suspicious tx patterns | **7 days** |
| **Low** | Informational, minor issue | Documentation error, config misconfiguration | **30 days** |

---

## 3. Alert Sources

### 3.1 Automated Monitoring

| Source | Metric | Alert Channel |
|--------|--------|---------------|
| Tracing logs | Error rate spikes | Log aggregation → Discord/Telegram |
| Peer monitoring | Active peers < 8 | PagerDuty (Stage 1+) |
| Mempool | Size > 80% capacity | Discord bot |
| Chain reorg | Depth > 3 blocks | PagerDuty (Stage 1+) |
| Invalid blocks | Rate > 5/10min per peer | Discord bot |
| Hash rate | Single miner > 33% | Community alert |

### 3.2 Manual Reports

| Source | Channel | SLA |
|--------|---------|-----|
| Bug Bounty (Immunefi) | immunefi.com/strangecoin | 48h acknowledgment |
| Security email | security@strangecoin.org | 48h acknowledgment |
| GitHub Issues | security label | 72h triage |
| Discord #security | Direct message to maintainers | 24h response |

### 3.3 External Notifications

| Source | Action |
|--------|--------|
| CVE databases | Monitor for relevant CVEs |
| Downstream dependencies | Alert if dependency compromised |
| Partner exchanges | Notify if chain integrity affected |

---

## 4. Response Team

### 4.1 Roles

| Role | Responsibility | Contact |
|------|---------------|---------|
| **Incident Commander** | Coordinates response, makes decisions | Lead maintainer |
| **Security Lead** | Technical investigation, fix development | Security maintainer |
| **Communications** | Public disclosure, community updates | Comms lead |
| **Infrastructure** | Node operations, network monitoring | Infra lead |

### 4.2 Escalation Path

```
1. Alert received → Security Lead triages (4h SLA)
2. If Critical → Incident Commander notified immediately
3. Incident Commander assembles response team
4. Fix developed → tested → deployed (emergency release if needed)
5. Post-incident review within 7 days
```

---

## 5. Response Procedures

### 5.1 Critical Incident (Active Exploit)

1. **Immediate (0-1h):**
   - Confirm incident (not false positive)
   - Notify Incident Commander
   - Assess scope: which chains affected, which funds at risk
   - If consensus bug: prepare emergency patch

2. **Short-term (1-4h):**
   - Develop fix (if code bug)
   - Coordinate with node operators for emergency upgrade
   - If 51% attack: alert exchanges to pause deposits/withdrawals
   - Prepare public statement

3. **Deployment (4-24h):**
   - Release emergency patch
   - Coordinate node upgrade (social consensus)
   - Monitor chain for stability
   - Resume normal operations

4. **Post-incident (1-7 days):**
   - Post-mortem analysis
   - Update threat model if new vector discovered
   - Update incident response procedures if needed
   - Community report

### 5.2 High Incident (Confirmed Vulnerability)

1. **Triage (0-24h):**
   - Confirm vulnerability
   - Assess impact: what's at risk
   - Develop proof-of-concept (internal only)

2. **Fix (1-7 days):**
   - Develop fix
   - Test on regtest/testnet
   - Prepare release

3. **Disclosure (7-90 days):**
   - Coordinated disclosure with stakeholders
   - Public disclosure after fix is deployed
   - CVE assignment if applicable

### 5.3 Medium Incident (Suspicious Activity)

1. **Investigation (0-7 days):**
   - Gather logs, analyze behavior
   - Determine if attack or misconfiguration
   - Document findings

2. **Response (if attack confirmed):**
   - Ban malicious peers
   - Update monitoring rules
   - Alert community if needed

---

## 6. Disclosure Timeline

### 6.1 Coordinated Disclosure

| Phase | Duration | Action |
|-------|----------|--------|
| **Discovery** | Day 0 | Vulnerability discovered/reported |
| **Triage** | Day 0-2 | Confirm, assess severity |
| **Fix Development** | Day 2-30 | Develop and test fix |
| **Stakeholder Notification** | Day 30-60 | Notify exchanges, node operators, partners |
| **Public Disclosure** | Day 60-90 | Publish advisory, release fix |
| **Post-Disclosure** | Day 90+ | Monitor for exploitation |

### 6.2 Disclosure Channels

| Channel | Content | Timing |
|---------|---------|--------|
| GitHub Advisory | Technical details | With fix release |
| Security mailing list | Summary + impact | With fix release |
| Discord #announcements | High-level summary | With fix release |
| Blog post | Detailed analysis | 7-14 days after fix |
| CVE database | CVE assignment | If applicable |

---

## 7. Communication Templates

### 7.1 Critical Incident (Initial)

```
Subject: [CRITICAL] Strangecoin Security Incident - [Brief Description]

We are investigating a critical security incident affecting Strangecoin.
[1-2 sentence description of impact.]

Actions taken:
- Incident response team assembled
- Investigating scope and impact
- Preparing emergency patch

Recommended actions for node operators:
- [Specific instructions]

Next update: [Time] UTC
```

### 7.2 Fix Release

```
Subject: [SECURITY] Strangecoin v[X.Y.Z] - Security Patch

A security vulnerability has been patched in v[X.Y.Z].

Impact: [Description]
Severity: [Critical/High/Medium]
Affected versions: [Versions]

Upgrade instructions:
1. [Steps]

Please upgrade immediately.
```

---

## 8. Post-Incident Review

### 8.1 Post-Mortem Template

1. **Timeline:** What happened, when, how it was detected
2. **Impact:** What was affected, scope of damage
3. **Root Cause:** Why it happened
4. **Response:** What was done, how long it took
5. **Lessons Learned:** What to improve
6. **Action Items:** Specific tasks with owners

### 8.2 Metrics

| Metric | Target |
|--------|--------|
| Time to detect | < 1 hour for critical |
| Time to triage | < 4 hours for critical |
| Time to fix | < 72 hours for critical |
| Time to disclose | < 90 days |
| Post-mortem completion | 100% for critical/high |

---

## 9. Testing & Drills

### 9.1 Tabletop Exercises

- Quarterly incident response drills
- Simulate critical scenarios (51% attack, consensus bug)
- Test communication channels
- Validate escalation procedures

### 9.2 Chaos Engineering

- (Stage 1+) Automated chaos testing
- Inject failures (network partition, disk failure)
- Validate monitoring and alerting

---

## 10. References

| Document | Purpose |
|----------|---------|
| `THREAT_MODEL.md` | Threat vectors and mitigations |
| `ARCHITECT3.md` §6 | Source threat model |
| `ROADMAP3.md` §Stage 0 Security | Security requirements |
| `docs/ADR/0003-hybrid-pow-pos.md` | Consensus decisions |
| Immunefi program | Bug bounty details |

---

**End of INCIDENT_RESPONSE.md.**
