// S2-P06 (strangler): the whole network subsystem lives in the
// `strangecoin-net` crate now; this module is a pure re-export shim so
// `crate::network::*` / `strangecoin::network::*` imports in the monolith,
// the GUI and the integration tests keep compiling unchanged.
#[cfg(feature = "net")]
pub use strangecoin_net::rate_limiter::{
    BucketSpec, MessageClass, MessageType, RateLimitConfig, RateLimitError, RateLimiter,
};
#[cfg(feature = "net")]
pub use strangecoin_net::{protocol, rate_limiter, sync, sync_engine};
