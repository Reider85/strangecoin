pub const CHAIN_ID_MAINNET: u32 = 1;
pub const CHAIN_ID_TESTNET: u32 = 2;
pub const CHAIN_ID_REGTEST: u32 = 3;

pub fn current_chain_id() -> u32 {
    CHAIN_ID_REGTEST
}