//! RFC 6238 time-based one-time codes for IBKR's Mobile Authenticator method.

use anyhow::{Result, bail};
use hmac::{Hmac, Mac};

/// Seconds per code (RFC 6238 default, used by IBKR).
pub const PERIOD: u64 = 30;

/// Decode the base32 secret shown when the Mobile Authenticator method is
/// enrolled. Spaces, hyphens, padding and letter case are ignored.
pub fn decode_secret(secret: &str) -> Result<Vec<u8>> {
    let digits: String = secret
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '='))
        .collect();
    if digits.len() <= 8 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        bail!(
            "The TOTP secret is a short number, which looks like a generated code. Supply the base32 secret from IBKR's Mobile Authenticator enrollment (letters A-Z and digits 2-7), not a six-digit code."
        );
    }
    let mut bits: u64 = 0;
    let mut count = 0;
    let mut out = Vec::with_capacity(digits.len() * 5 / 8);
    for c in digits.chars() {
        let value = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u64 - 'A' as u64,
            c @ '2'..='7' => c as u64 - '2' as u64 + 26,
            _ => bail!(
                "The TOTP secret is not base32. Supply the secret from IBKR's Mobile Authenticator enrollment, which uses only the letters A-Z and the digits 2-7."
            ),
        };
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if out.len() < 10 {
        bail!(
            "The TOTP secret is shorter than 80 bits. Copy the complete base32 secret from IBKR's Mobile Authenticator enrollment."
        );
    }
    Ok(out)
}

/// Six-digit code for `key` at Unix time `unix_secs`, and the seconds the
/// code stays valid.
pub fn code(key: &[u8], unix_secs: u64) -> (String, u64) {
    let counter = unix_secs / PERIOD;
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    (
        format!("{:06}", value % 1_000_000),
        PERIOD - unix_secs % PERIOD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_6238_sha1_vectors() {
        // RFC 6238 Appendix B, SHA-1 key "12345678901234567890"; the RFC
        // lists eight digits, of which a six-digit code is the last six.
        let key = b"12345678901234567890";
        for (time, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code(key, time).0, expected, "time {time}");
        }
        assert_eq!(code(key, 59).1, 1);
        assert_eq!(code(key, 60).1, 30);
    }

    #[test]
    fn secrets_decode_and_bad_input_explains_the_fix() {
        // "12345678901234567890" in base32, with spacing and lower case.
        assert_eq!(
            decode_secret("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").unwrap(),
            b"12345678901234567890"
        );
        let short = decode_secret("123456").unwrap_err().to_string();
        assert!(short.contains("generated code"), "{short}");
        let bad = decode_secret("not*base32").unwrap_err().to_string();
        assert!(bad.contains("not base32"), "{bad}");
        assert!(decode_secret("GEZDGNBV").is_err(), "too short");
    }
}
