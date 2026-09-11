use secp256k1::PublicKey;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

pub fn address_from_public_key(pk: &PublicKey) -> String {
    BASE64.encode(pk.serialize())
}