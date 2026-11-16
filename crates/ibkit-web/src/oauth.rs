//! OAuth 1.0a for the IBKR Web API (first-party and third-party OAuth).
//!
//! The flow, as IBKR implements it:
//!
//! 1. Decrypt the access token secret with the private *encryption* key
//!    (RSA PKCS#1 v1.5). Its hex form is the "prepend".
//! 2. Pick a 256-bit random `a`, send the Diffie-Hellman challenge `g^a mod p`
//!    in a POST to `oauth/live_session_token`, signed with RSA-SHA256 by the
//!    private *signature* key over `prepend + base string`.
//! 3. The response carries `diffie_hellman_response` `B`. The shared secret is
//!    `K = B^a mod p`; the live session token is
//!    `base64(HMAC-SHA1(key = bytes(K), msg = bytes(prepend)))`.
//! 4. Later requests are signed with HMAC-SHA256 keyed by the decoded token.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey};
use std::collections::BTreeMap;

/// OAuth errors.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error(
        "the private key is not valid PKCS#1 or PKCS#8 PEM: {0}. Pass the PEM text of the key, including the BEGIN/END lines"
    )]
    Key(String),
    #[error("RSA operation failed: {0}")]
    Rsa(String),
    #[error("{field} is not valid {expected}: {value:?}")]
    Format {
        field: &'static str,
        expected: &'static str,
        value: String,
    },
    #[error("{0}")]
    Invalid(String),
}

/// OAuth 1.0a credentials from the IBKR self-service portal.
#[derive(Clone, Debug)]
pub struct OAuthConfig {
    pub consumer_key: String,
    pub access_token: String,
    /// Base64 access token secret, encrypted with the public encryption key.
    pub access_token_secret: String,
    /// PEM private key used to decrypt the access token secret.
    pub encryption_key_pem: String,
    /// PEM private key used for RSA-SHA256 signatures.
    pub signature_key_pem: String,
    /// Diffie-Hellman prime as hex.
    pub dh_prime: String,
    pub dh_generator: u32,
    /// `"limited_poa"` for first-party OAuth, `"test_realm"` for the test consumer.
    pub realm: String,
}

impl OAuthConfig {
    pub fn new(
        consumer_key: &str,
        access_token: &str,
        access_token_secret: &str,
        encryption_key_pem: &str,
        signature_key_pem: &str,
        dh_prime: &str,
    ) -> Self {
        Self {
            consumer_key: consumer_key.into(),
            access_token: access_token.into(),
            access_token_secret: access_token_secret.into(),
            encryption_key_pem: encryption_key_pem.into(),
            signature_key_pem: signature_key_pem.into(),
            dh_prime: dh_prime.into(),
            dh_generator: 2,
            realm: "limited_poa".into(),
        }
    }
}

/// Parse a PKCS#1 or PKCS#8 PEM private key.
pub fn read_private_key(pem: &str) -> Result<RsaPrivateKey, OAuthError> {
    RsaPrivateKey::from_pkcs1_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs8_pem(pem))
        .map_err(|e| OAuthError::Key(e.to_string()))
}

/// Python `urllib.parse.quote_plus`: keep `A-Za-z0-9_.-~`, space as `+`.
pub fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 16 random alphanumeric characters.
pub fn nonce() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..16)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

/// OAuth signature base string: `METHOD&quote(url)&quote(sorted k=v joined by &)`,
/// optionally prefixed by `prepend`.
pub fn base_string(
    method: &str,
    url: &str,
    params: &BTreeMap<String, String>,
    prepend: Option<&str>,
) -> String {
    let joined: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let s = format!(
        "{method}&{}&{}",
        quote_plus(url),
        quote_plus(&joined.join("&"))
    );
    match prepend {
        Some(p) => format!("{p}{s}"),
        None => s,
    }
}

/// Hex of the decrypted access token secret.
pub fn live_session_token_prepend(
    access_token_secret_b64: &str,
    encryption_key: &RsaPrivateKey,
) -> Result<String, OAuthError> {
    let secret = B64
        .decode(access_token_secret_b64.trim())
        .map_err(|_| OAuthError::Format {
            field: "access_token_secret",
            expected: "base64",
            value: access_token_secret_b64.into(),
        })?;
    let plain = encryption_key
        .decrypt(Pkcs1v15Encrypt, &secret)
        .map_err(|e| OAuthError::Rsa(e.to_string()))?;
    Ok(hex(&plain))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hex(field: &'static str, s: &str) -> Result<BigUint, OAuthError> {
    BigUint::parse_bytes(s.trim().as_bytes(), 16).ok_or_else(|| OAuthError::Format {
        field,
        expected: "hex",
        value: s.into(),
    })
}

/// A random 256-bit Diffie-Hellman exponent as hex.
pub fn dh_random() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    BigUint::from_bytes_be(&b).to_str_radix(16)
}

/// `g^random mod prime` as hex.
pub fn dh_challenge(
    prime_hex: &str,
    random_hex: &str,
    generator: u32,
) -> Result<String, OAuthError> {
    let p = parse_hex("dh_prime", prime_hex)?;
    let a = parse_hex("dh_random", random_hex)?;
    Ok(BigUint::from(generator).modpow(&a, &p).to_str_radix(16))
}

/// Big-endian bytes of `x` with a leading zero byte when the top bit is set
/// (Java `BigInteger.toByteArray`, as IBKR computes it).
pub fn to_byte_array(x: &BigUint) -> Vec<u8> {
    let mut b = x.to_bytes_be();
    if x.bits().is_multiple_of(8) {
        b.insert(0, 0);
    }
    b
}

/// Derive the live session token from the server's DH response.
pub fn live_session_token(
    prime_hex: &str,
    random_hex: &str,
    dh_response_hex: &str,
    prepend_hex: &str,
) -> Result<String, OAuthError> {
    let p = parse_hex("dh_prime", prime_hex)?;
    let a = parse_hex("dh_random", random_hex)?;
    let b = parse_hex("diffie_hellman_response", dh_response_hex)?;
    let k = b.modpow(&a, &p);
    let prepend = (0..prepend_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&prepend_hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| OAuthError::Format {
            field: "prepend",
            expected: "hex",
            value: prepend_hex.into(),
        })?;
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&to_byte_array(&k))
        .expect("HMAC accepts any key length");
    mac.update(&prepend);
    Ok(B64.encode(mac.finalize().into_bytes()))
}

/// True when the server's signature confirms the token.
pub fn validate_live_session_token(
    token_b64: &str,
    signature_hex: &str,
    consumer_key: &str,
) -> bool {
    let Ok(key) = B64.decode(token_b64) else {
        return false;
    };
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(consumer_key.as_bytes());
    hex(&mac.finalize().into_bytes()) == signature_hex
}

/// URL-quoted base64 RSA-SHA256 PKCS#1 v1.5 signature.
pub fn rsa_sha256_signature(base_string: &str, key: &RsaPrivateKey) -> String {
    let signer = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(key.clone());
    quote_plus(&B64.encode(signer.sign(base_string.as_bytes()).to_bytes()))
}

/// URL-quoted base64 HMAC-SHA256 signature keyed by the live session token.
pub fn hmac_sha256_signature(
    base_string: &str,
    live_session_token_b64: &str,
) -> Result<String, OAuthError> {
    let key = B64
        .decode(live_session_token_b64)
        .map_err(|_| OAuthError::Format {
            field: "live_session_token",
            expected: "base64",
            value: live_session_token_b64.into(),
        })?;
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(base_string.as_bytes());
    Ok(quote_plus(&B64.encode(mac.finalize().into_bytes())))
}

/// `OAuth realm="..", k="v", ...` with keys sorted.
pub fn authorization_header(params: &BTreeMap<String, String>, realm: &str) -> String {
    let kv: Vec<String> = params.iter().map(|(k, v)| format!("{k}=\"{v}\"")).collect();
    format!("OAuth realm=\"{realm}\", {}", kv.join(", "))
}

/// Signing mode of one request.
pub enum Signing<'a> {
    /// RSA-SHA256 with a prepend, used only for the live session token request.
    Rsa {
        key: &'a RsaPrivateKey,
        prepend: &'a str,
    },
    /// HMAC-SHA256 with the live session token.
    Hmac { live_session_token: &'a str },
}

/// Build the Authorization header for one request.
pub fn sign_request(
    cfg: &OAuthConfig,
    method: &str,
    url: &str,
    query: &[(String, String)],
    extra: &[(&str, String)],
    signing: Signing<'_>,
) -> Result<String, OAuthError> {
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    params.insert("oauth_consumer_key".into(), cfg.consumer_key.clone());
    params.insert("oauth_nonce".into(), nonce());
    params.insert(
        "oauth_signature_method".into(),
        match signing {
            Signing::Rsa { .. } => "RSA-SHA256".into(),
            Signing::Hmac { .. } => "HMAC-SHA256".into(),
        },
    );
    params.insert(
        "oauth_timestamp".into(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
            .to_string(),
    );
    if !cfg.access_token.is_empty() {
        params.insert("oauth_token".into(), cfg.access_token.clone());
    }
    for (k, v) in extra {
        params.insert((*k).into(), v.clone());
    }
    let mut signed = params.clone();
    for (k, v) in query {
        signed.insert(k.clone(), v.clone());
    }
    let signature = match signing {
        Signing::Rsa { key, prepend } => {
            rsa_sha256_signature(&base_string(method, url, &signed, Some(prepend)), key)
        }
        Signing::Hmac { live_session_token } => {
            hmac_sha256_signature(&base_string(method, url, &signed, None), live_session_token)?
        }
    };
    params.insert("oauth_signature".into(), signature);
    Ok(authorization_header(&params, &cfg.realm))
}
