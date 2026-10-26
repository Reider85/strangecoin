//! S2-P11 — DNSSEC-validated DNS seed resolution (threat V-16: eclipse via
//! DNS poisoning, ARCHITECT3 §6 «Hard-coded seed IPs (not just DNS);
//! DNSSEC validation; rotation of seeds»).
//!
//! Production path ([`DnssecResolver`]) is a hickory stub resolver with
//! `ResolverOpts::validate = true`: the DNSSEC chain is verified against
//! the built-in root trust anchors, a tampered chain fails the lookup with
//! `DnssecBogus`, and every returned record carries a per-record
//! [`Proof`]. [`apply_dnssec_policy`] is the pure, unit-testable gate the
//! bootstrap applies per [`SeedConfig::dnssec_required`]: when required,
//! anything short of `Proof::Secure` is rejected.
//!
//! Tests never touch the network: they feed [`DnsAnswer`] values straight
//! into [`apply_dnssec_policy`] / [`super::bootstrap`] through a mock
//! resolver closure.

use crate::error::NetError;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::dnssec::Proof;
use hickory_resolver::Resolver;
use hickory_resolver::TokioResolver;
use std::net::IpAddr;
use tracing::warn;

/// Result of resolving one seed domain.
///
/// `validated` is `true` only when **every** answer record carries
/// [`Proof::Secure`] — unsigned (`Insecure`), unverifiable
/// (`Indeterminate`) and failed (`Bogus`, which surfaces as an error in
/// production) answers all arrive as `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswer {
    pub addrs: Vec<IpAddr>,
    pub validated: bool,
}

impl DnsAnswer {
    pub fn empty() -> Self {
        DnsAnswer {
            addrs: Vec::new(),
            validated: false,
        }
    }
}

/// Production DNSSEC-validating resolver.
///
/// Built from the system resolver configuration when available (falls back
/// to the public Google resolvers otherwise); validation is enabled by
/// `opts.validate = true` with the built-in root trust anchors — no manual
/// anchor installation.
pub struct DnssecResolver {
    resolver: TokioResolver,
}

impl DnssecResolver {
    /// Construct the validating stub resolver.
    ///
    /// Pure setup (config parsing + handle creation), no network I/O — the
    /// first real query happens in [`Self::resolve`].
    pub fn new() -> Result<Self, NetError> {
        let mut builder = match Resolver::builder_tokio() {
            Ok(builder) => builder,
            Err(err) => {
                warn!(
                    target = "net::discovery",
                    error = %err,
                    "system resolver config unavailable, falling back to public resolvers"
                );
                Resolver::builder_with_config(
                    ResolverConfig::udp_and_tcp(&hickory_resolver::config::GOOGLE),
                    TokioRuntimeProvider::default(),
                )
            }
        };
        builder.options_mut().validate = true;
        let resolver = builder
            .build()
            .map_err(|err| NetError::Discovery(format!("building DNSSEC resolver: {err}")))?;
        Ok(Self { resolver })
    }

    /// Resolve `domain` to IPs, tagging the answer with whether the whole
    /// RRset chain validated. Failures (including `DnssecBogus` on a
    /// tampered chain) degrade to an unvalidated empty answer — the policy
    /// gate decides whether that is fatal.
    pub async fn resolve(&self, domain: &str) -> DnsAnswer {
        let lookup = match self.resolver.lookup_ip(domain).await {
            Ok(lookup) => lookup,
            Err(err) => {
                warn!(
                    target = "net::discovery",
                    domain = %domain,
                    error = %err,
                    "DNS seed resolution failed"
                );
                return DnsAnswer::empty();
            }
        };
        let answers = lookup.as_lookup().answers();
        let validated = !answers.is_empty() && answers.iter().all(|r| r.proof == Proof::Secure);
        DnsAnswer {
            addrs: lookup.iter().collect(),
            validated,
        }
    }
}

/// The DNSSEC gate: reject answers that did not validate when the config
/// demands DNSSEC (`dnssec_required = true`), accept everything otherwise.
///
/// This is the V-16 mitigation in its purest form — the bootstrap never
/// sees the addresses of a poisoned answer.
pub fn apply_dnssec_policy(answer: &DnsAnswer, required: bool) -> Result<Vec<IpAddr>, NetError> {
    if answer.addrs.is_empty() {
        return Err(NetError::Discovery(
            "DNS seed resolved to no addresses".into(),
        ));
    }
    if required && !answer.validated {
        return Err(NetError::Discovery(
            "DNSSEC validation required but answer is not Secure".into(),
        ));
    }
    Ok(answer.addrs.clone())
}

/// Prompt entry point: resolve `domain` and refuse anything that is not
/// DNSSEC-`Secure`.
pub async fn resolve_with_dnssec(domain: &str) -> Result<Vec<IpAddr>, NetError> {
    let resolver = DnssecResolver::new()?;
    let answer = resolver.resolve(domain).await;
    apply_dnssec_policy(&answer, true)
}
