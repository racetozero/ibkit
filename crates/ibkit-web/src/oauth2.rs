//! OAuth 2.0 (`private_key_jwt`, RFC 7521/7523) for the IBKR Web API.
//!
//! 1. `POST {auth_url}/oauth2/api/v1/token` with a form carrying a JWT client
//!    assertion (claims `iss`, `sub` = client id, `aud` = `/token`, a 30 s
//!    window). The answer's `access_token` authorizes step 2.
//! 2. `POST {auth_url}/gw/api/v1/sso-sessions` with `Authorization: Bearer
//!    <access token>` and a JWT body (claims `ip`, `credential` = username,
//!    `iss`, 24 h expiry), content type `application/jwt`. The answer's
//!    `access_token` is the bearer token for `/v1/api` requests and the
//!    WebSocket (`bearer_token` query parameter).
//!
//! Both JWTs are RS256 with header `{"alg":"RS256","typ":"JWT","kid":<key id>}`.

use crate::oauth::{OAuthError, read_private_key};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use rsa::RsaPrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use serde_json::{Value, json};

/// Default host of the OAuth 2.0 token and SSO session endpoints.
pub const AUTH_URL: &str = "https://api.ibkr.com";
/// Default scope of the access token.
pub const SCOPE: &str = "sso-sessions.write";

/// OAuth 2.0 credentials registered with IBKR.
#[derive(Clone, Debug)]
pub struct OAuth2Config {
    pub client_id: String,
    /// `kid` of the registered public key.
    pub key_id: String,
    /// IBKR username the session is for.
    pub credential: String,
    /// PEM (PKCS#1 or PKCS#8) private key that signs the assertions.
    pub private_key_pem: String,
    /// Public IP address the requests come from; IBKR checks it against the connection.
    pub ip: String,
    /// Space-separated scopes of the access token. Trading needs
    /// `sso-sessions.write`; add the scopes of the Account Management
    /// endpoints you call (for example `accounts.read accounts.write`).
    pub scope: String,
    /// Host of the token, session and `/gw/api` endpoints
    /// (`https://qa.interactivebrokers.com` for the sandbox).
    pub auth_url: String,
}

impl OAuth2Config {
    pub fn new(
        client_id: &str,
        key_id: &str,
        credential: &str,
        private_key_pem: &str,
        ip: &str,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            key_id: key_id.into(),
            credential: credential.into(),
            private_key_pem: private_key_pem.into(),
            ip: ip.into(),
            scope: SCOPE.into(),
            auth_url: AUTH_URL.into(),
        }
    }

    pub fn private_key(&self) -> Result<RsaPrivateKey, OAuthError> {
        read_private_key(&self.private_key_pem)
    }
}

/// Compact RS256 JWT of `claims`.
pub fn jwt(key_id: &str, claims: &Value, key: &RsaPrivateKey) -> String {
    let header = json!({"alg": "RS256", "typ": "JWT", "kid": key_id});
    let payload = format!(
        "{}.{}",
        B64URL.encode(header.to_string()),
        B64URL.encode(claims.to_string())
    );
    let signer = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(key.clone());
    let signature = signer.sign(payload.as_bytes()).to_bytes();
    format!("{payload}.{}", B64URL.encode(signature))
}

/// Client assertion for the token request, issued at `now` (Unix seconds).
pub fn access_token_assertion(cfg: &OAuth2Config, key: &RsaPrivateKey, now: i64) -> String {
    let claims = json!({
        "iss": cfg.client_id,
        "sub": cfg.client_id,
        "aud": "/token",
        "exp": now + 20,
        "iat": now - 10,
    });
    jwt(&cfg.key_id, &claims, key)
}

/// Body of a request with the "Signed JWT" security policy (every POST and
/// PATCH under `/gw/api`): the JSON fields of `body` plus `iss` (client id),
/// `iat` (`now`, Unix seconds) and `exp` (`lifetime` seconds later), sent as
/// `application/jwt`. A `body` that is not a JSON object is an error.
pub fn signed_body(
    cfg: &OAuth2Config,
    key: &RsaPrivateKey,
    body: &Value,
    now: i64,
    lifetime: i64,
) -> Result<String, OAuthError> {
    let mut claims = match body {
        Value::Object(m) => m.clone(),
        Value::Null => serde_json::Map::new(),
        other => {
            return Err(OAuthError::Invalid(format!(
                "a Signed JWT request body must be a JSON object, got {other}; pass the documented request object"
            )));
        }
    };
    claims.insert("iss".into(), json!(cfg.client_id));
    claims.insert("exp".into(), json!(now + lifetime));
    claims.insert("iat".into(), json!(now));
    Ok(jwt(&cfg.key_id, &Value::Object(claims), key))
}

/// Form fields of `POST /oauth2/api/v1/token`.
pub fn token_form(cfg: &OAuth2Config, assertion: &str) -> [(&'static str, String); 4] {
    [
        ("grant_type", "client_credentials".into()),
        (
            "client_assertion_type",
            "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".into(),
        ),
        ("client_assertion", assertion.into()),
        ("scope", cfg.scope.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::signature::Verifier;

    #[test]
    fn assertions_carry_the_documented_claims_and_verify() {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024).expect("key");
        let cfg = OAuth2Config::new("client", "kid-1", "user1", "", "1.2.3.4");
        let token = access_token_assertion(&cfg, &key, 1_000);
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |p: &str| -> Value {
            serde_json::from_slice(&B64URL.decode(p).expect("base64url")).expect("json")
        };
        assert_eq!(
            decode(parts[0]),
            json!({"alg": "RS256", "typ": "JWT", "kid": "kid-1"})
        );
        assert_eq!(
            decode(parts[1]),
            json!({"iss": "client", "sub": "client", "aud": "/token", "exp": 1020, "iat": 990})
        );
        let verifier = VerifyingKey::<sha2::Sha256>::new(key.to_public_key());
        let sig =
            Signature::try_from(B64URL.decode(parts[2]).expect("sig").as_slice()).expect("sig");
        verifier
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .expect("signature verifies with the public key");

        // The SSO session body, as oauth2_init sends it.
        let session = signed_body(
            &cfg,
            &key,
            &json!({"ip": cfg.ip, "credential": cfg.credential}),
            1_000,
            86400,
        )
        .expect("an object body");
        let claims = decode(session.split('.').nth(1).expect("claims"));
        assert_eq!(
            claims,
            json!({"ip": "1.2.3.4", "credential": "user1", "iss": "client", "exp": 87400, "iat": 1000})
        );
    }
}
