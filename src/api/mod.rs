//! API-слой P02 (скелет Stage 0). Сознательно пуст до Stage 4:
//! JSON-RPC eth_*-совместимый слой (`eth_sendTransaction`, `eth_getBalance`,
//! `eth_call`, `eth_getLogs`, `eth_blockNumber`, `eth_subscribe` + sc_*-методы)
//! отнесён ROADMAP3 к Stage 4 (Dev-experience) и ARCHITECT3 §3.10/§10.6 —
//! тогда же появится отдельный крейт `strangecoin-api`.
//! Не реализовывать здесь до Stage 4 (BUG-S0-020).
