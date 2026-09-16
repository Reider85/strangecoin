use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use secp256k1::PublicKey;

pub fn address_from_public_key(pk: &PublicKey) -> String {
    BASE64.encode(pk.serialize())
}
