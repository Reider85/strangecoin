use crate::consensus;
use crate::error::CoreError;
use bech32::{decode, encode, Bech32m, Hrp};
use secp256k1::PublicKey;

pub fn hrp_for_network(network_id: u32) -> Result<&'static str, CoreError> {
    match network_id {
        consensus::CHAIN_ID_MAINNET => Ok("sc"),
        consensus::CHAIN_ID_TESTNET => Ok("tsc"),
        consensus::CHAIN_ID_REGTEST => Ok("rsc"),
        _ => Err(CoreError::UnknownNetworkId { network_id }),
    }
}

pub fn encode_address(pk: &PublicKey, network_id: u32) -> Result<String, CoreError> {
    let hrp = hrp_for_network(network_id)?;
    let hrp = Hrp::parse(hrp).map_err(|_| CoreError::InvalidAddressFormat {
        reason: format!("Invalid HRP: {}", hrp),
    })?;

    let pk_bytes = pk.serialize();
    let data = pk_bytes.as_slice();
    let encoded = encode::<Bech32m>(hrp, data)?;

    Ok(encoded)
}

pub fn decode_address(s: &str) -> Result<(PublicKey, u32), CoreError> {
    let (hrp, data) = decode(s).map_err(|_| CoreError::InvalidAddressChecksum)?;
    let hrp_str = hrp.as_str();

    let network_id = match hrp_str {
        "sc" => consensus::CHAIN_ID_MAINNET,
        "tsc" => consensus::CHAIN_ID_TESTNET,
        "rsc" => consensus::CHAIN_ID_REGTEST,
        _ => {
            return Err(CoreError::UnknownAddressHrp {
                hrp: hrp_str.to_string(),
            })
        }
    };

    let bytes = data;

    let pk = PublicKey::from_slice(&bytes).map_err(|e| CoreError::InvalidAddressFormat {
        reason: format!("Invalid public key: {}", e),
    })?;

    Ok((pk, network_id))
}

/// Deterministic contract address derivation (S1.5-P08): the address of the
/// contract deployed by `creator_addr` at nonce `nonce` is the bech32m encoding
/// of `blake3("sc-contract-addr" || creator || nonce)` with the network HRP.
/// The 32-byte payload cannot collide with a 33-byte EOA pubkey encoding.
pub fn derive_contract_address(
    creator_addr: &str,
    nonce: u64,
    network_id: u32,
) -> Result<String, CoreError> {
    let hrp = hrp_for_network(network_id)?;
    let hrp = Hrp::parse(hrp).map_err(|_| CoreError::InvalidAddressFormat {
        reason: format!("Invalid HRP: {}", hrp),
    })?;

    let mut input = Vec::with_capacity(15 + creator_addr.len() + 8);
    input.extend_from_slice(b"sc-contract-addr");
    input.extend_from_slice(creator_addr.as_bytes());
    input.extend_from_slice(&nonce.to_be_bytes());
    let hash = blake3::hash(&input);

    Ok(encode::<Bech32m>(hrp, hash.as_bytes())?)
}

/// Reserved precompile addresses `0x01..=0x07` (ADR-0015). `create` must never
/// deploy into this range; full precompile dispatch lands in S1.5-P11.
pub fn is_precompile_address(addr: &str) -> bool {
    matches!(
        addr,
        "0x01" | "0x02" | "0x03" | "0x04" | "0x05" | "0x06" | "0x07"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;
    use secp256k1::{Secp256k1, SecretKey};

    #[test]
    fn test_round_trip() {
        let secp = Secp256k1::new();
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);

        for network_id in [1, 2, 3] {
            let encoded = encode_address(&pk, network_id).unwrap();
            let (decoded_pk, decoded_net) = decode_address(&encoded).unwrap();

            assert_eq!(decoded_pk, pk);
            assert_eq!(decoded_net, network_id);
        }
    }

    #[test]
    fn test_checksum_error() {
        let bad_address = "sc1invalidaddress12345";
        assert!(matches!(
            decode_address(bad_address),
            Err(CoreError::InvalidAddressChecksum)
        ));
    }

    #[test]
    fn test_hrp_validation() {
        let secp = Secp256k1::new();
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);

        // Valid HRP
        assert!(encode_address(&pk, 1).is_ok());
        assert!(encode_address(&pk, 2).is_ok());
        assert!(encode_address(&pk, 3).is_ok());

        // Invalid network ID
        assert!(matches!(
            encode_address(&pk, 99),
            Err(CoreError::UnknownNetworkId { network_id: 99 })
        ));

        // Invalid HRP in decode - use a valid bech32 string with unknown HRP
        // We need to encode a proper bech32 string with an unknown HRP to test UnknownAddressHrp
        let _unknown_hrp_address = "unknown1address";
        // For now, test that invalid checksum gives InvalidAddressChecksum
        let invalid_checksum_address = "sc1invalidaddress12345";
        match decode_address(invalid_checksum_address) {
            Err(CoreError::InvalidAddressChecksum) => (),
            Err(e) => panic!("Expected InvalidAddressChecksum error, got: {:?}", e),
            _ => panic!("Expected InvalidAddressChecksum error"),
        }
    }

    #[test]
    fn test_network_id_hrp_mapping() {
        let secp = Secp256k1::new();
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);

        // Test that encode produces addresses with correct HRP
        let mainnet_addr = encode_address(&pk, 1).unwrap();
        assert!(mainnet_addr.starts_with("sc1"));

        let testnet_addr = encode_address(&pk, 2).unwrap();
        assert!(testnet_addr.starts_with("tsc1"));

        let regtest_addr = encode_address(&pk, 3).unwrap();
        assert!(regtest_addr.starts_with("rsc1"));
    }

    #[test]
    fn test_derive_contract_address_deterministic() {
        let a1 = derive_contract_address("rsc1creator", 1, 3).unwrap();
        let a2 = derive_contract_address("rsc1creator", 1, 3).unwrap();
        assert_eq!(a1, a2, "same (creator, nonce) must derive the same address");
        assert!(a1.starts_with("rsc1"), "regtest HRP expected, got {a1}");

        let a3 = derive_contract_address("rsc1creator", 2, 3).unwrap();
        assert_ne!(a1, a3, "different nonce must derive a different address");

        let a4 = derive_contract_address("rsc1other", 1, 3).unwrap();
        assert_ne!(a1, a4, "different creator must derive a different address");

        let mainnet = derive_contract_address("sc1creator", 1, 1).unwrap();
        assert!(mainnet.starts_with("sc1"));
    }

    #[test]
    fn test_contract_address_is_not_an_eoa() {
        let contract = derive_contract_address("rsc1creator", 0, 3).unwrap();
        assert!(
            decode_address(&contract).is_err(),
            "32-byte contract payload must not decode as a 33-byte EOA pubkey"
        );
    }

    #[test]
    fn test_is_precompile_address() {
        assert!(is_precompile_address("0x01"));
        assert!(is_precompile_address("0x07"));
        assert!(!is_precompile_address("0x08"));
        assert!(!is_precompile_address("rsc1something"));
    }
}
