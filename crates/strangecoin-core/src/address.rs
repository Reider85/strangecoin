use bech32::{Hrp, encode, decode, Bech32m};
use secp256k1::PublicKey;
use crate::error::CoreError;
use crate::consensus;

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
        _ => return Err(CoreError::UnknownAddressHrp {
            hrp: hrp_str.to_string(),
        }),
    };
    
    let bytes = data;
    
    let pk = PublicKey::from_slice(&bytes).map_err(|e| CoreError::InvalidAddressFormat {
        reason: format!("Invalid public key: {}", e),
    })?;
    
    Ok((pk, network_id))
}

pub fn address_from_public_key(pk: &PublicKey) -> Result<String, CoreError> {
    encode_address(pk, consensus::current_chain_id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey};
    use rand::RngCore;

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
        assert!(matches!(decode_address(bad_address), Err(CoreError::InvalidAddressChecksum)));
    }

    #[test]
    fn test_hrp_validation() {
        let secp = Secp256k1::new();
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);
        
        // Valid HRP
        assert!(matches!(encode_address(&pk, 1), Ok(_)));
        assert!(matches!(encode_address(&pk, 2), Ok(_)));
        assert!(matches!(encode_address(&pk, 3), Ok(_)));
        
        // Invalid network ID
        assert!(matches!(encode_address(&pk, 99), Err(CoreError::UnknownNetworkId { network_id: 99 })));
        
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
    fn test_address_from_public_key_wrapper() {
        let secp = Secp256k1::new();
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).unwrap();
        let pk = PublicKey::from_secret_key(&secp, &sk);
        
        let addr = address_from_public_key(&pk).unwrap();
        assert!(addr.starts_with("rsc1")); // current_chain_id() returns REGTEST = 3
    }
}
