# SECURITY.md — Strangecoin Security Policy

**Version:** 1.1
**Date:** 2026-11-09
**Status:** Active

---

## 1. Security Contacts

**Security Policy**: We take security seriously. If you discover a security vulnerability, please report it responsibly.

**Vulnerability Bounty Program**: We offer bounties for critical security vulnerabilities. See [BOUNTY.md](docs/security/BOUNTY.md) for details.

**Contact Methods**:
- **Private Security Reports**: security@strangecoin.dev (private email)
- **GitHub Security Advisories**: [Report via GitHub](https://github.com/Reider85/strangecoin/security/advisories)
- **Discussions**: [Security category](https://github.com/Reider85/strangecoin/discussions/categories/q-a)

---

## 2. Supported Versions

| Version | Status | Support Type |
|---------|--------|--------------|
| v2.0.0-stage2 | ✅ Current | Full support |
| v1.5.0-stage1.5 | ❌ Deprecated | Security patches only |
| < v1.5.0 | ❌ Unsupported | No support |

---

## 3. Reporting a Vulnerability

### How to Report

1. **Private Disclosure**: Please report vulnerabilities privately to avoid public exposure
2. **GitHub Security Advisories**: Use [GitHub's private vulnerability reporting](https://github.com/Reider85/strangecoin/security/advisories)
3. **Include Details**: Provide detailed information about the vulnerability

### Required Information

When reporting a vulnerability, please include:

- **Description**: Clear description of the vulnerability
- **Steps to Reproduce**: Steps to reproduce the issue
- **Expected vs Actual Behavior**: What should happen vs what actually happens
- **Impact**: Potential impact on the system
- **Environment**: OS, Rust version, Strangecoin version, etc.
- **Proof of Concept**: Code or steps that demonstrate the vulnerability

### Response Time

- **Critical**: Within 24 hours
- **High**: Within 48 hours
- **Medium**: Within 72 hours
- **Low**: Within 1 week

---

## 4. Security Scope

### In Scope

- Core blockchain consensus code
- Cryptographic implementations (secp256k1, blake3, etc.)
- Network protocol (Noise, P2P)
- WASM smart contract execution
- Storage engine (RocksDB)
- API endpoints and RPC interfaces
- Key management and authentication

### Out of Scope

- Third-party dependencies (unless they are part of the core distribution)
- Operating system vulnerabilities
- Network infrastructure
- User applications built on Strangecoin
- Documentation examples (unless they demonstrate security issues)

---

## 5. Security Best Practices

### For Users

- Use testnet only (mainnet not yet available)
- Keep your software updated to the latest version
- Use secure key management practices
- Verify network connections before sending transactions
- Monitor for unusual activity

### for Developers

- Follow secure coding practices
- Review code changes for security implications
- Test thoroughly before merging
- Use static analysis tools
- Keep dependencies updated

---

## 6. Security Incidents

### Incident Response

1. **Containment**: Immediate isolation of affected systems
2. **Investigation**: Determine root cause and impact
3. **Mitigation**: Implement fixes and patches
4. **Communication**: Notify affected parties
5. **Prevention**: Implement measures to prevent recurrence

### Past Incidents

- **2026-10-13**: Genesis key burn announcement (SCIP-0001)
- **2026-09-15**: Noise protocol security audit completed
- **2026-08-20**: WASM sandbox security review

---

## 7. Security Audits

### Completed Audits

- **Noise Protocol Framework**: Third-party audit completed (2026-10)
- **WASM Smart Contracts**: Internal security review (2026-09)
- **Cryptographic Implementations**: secp256k1 bindings audit (2026-08)

### Planned Audits

- **Full Network Stack**: Scheduled for Q1 2027
- **Storage Engine**: RocksDB security review (planned)
- **Smart Contract VM**: Comprehensive audit (planned)

---

## 8. Resources

### Documentation

- [Security Guidelines](docs/security/SECURITY_GUIDELINES.md)
- [Vulnerability Bounty Program](docs/security/BOUNTY.md)
- [Security Best Practices](docs/security/BEST_PRACTICES.md)

### Tools

- [Security Testing Tools](docs/security/TOOLS.md)
- [Vulnerability Disclosure Process](docs/security/DISCLOSURE.md)
- [Incident Response Plan](docs/security/INCIDENT_RESPONSE.md)

---

*Last updated: 2026-11-09*