//! Web Push: notifications on phones and desktops even when delune isn't open.
//!
//! A browser that subscribes hands over an endpoint (its push service) and two keys.
//! Each message is encrypted for that browser (RFC 8291, `aes128gcm`) and the request
//! is signed with delune's own key pair (VAPID, RFC 8292) so the push service knows who
//! sent it. Everything uses aws-lc-rs, which rustls already brings in.

use aws_lc_rs::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey, agree_ephemeral};
use aws_lc_rs::hmac;
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// Record size advertised in the header; messages are one record.
const RECORD_SIZE: u32 = 4096;

/// delune's VAPID key pair, kept as PKCS#8.
pub struct Vapid {
    pair: EcdsaKeyPair,
}

impl std::fmt::Debug for Vapid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vapid").field("public_key", &self.public_key()).finish_non_exhaustive()
    }
}

impl Vapid {
    /// A new key pair, and its PKCS#8 form to save.
    ///
    /// # Errors
    ///
    /// When the system can't produce random numbers.
    pub fn generate() -> Result<(Self, Vec<u8>), String> {
        let rng = SystemRandom::new();
        let document =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).map_err(|e| e.to_string())?;
        let bytes = document.as_ref().to_vec();
        Ok((Self::from_pkcs8(&bytes)?, bytes))
    }

    /// # Errors
    ///
    /// When the bytes aren't a P-256 private key.
    pub fn from_pkcs8(bytes: &[u8]) -> Result<Self, String> {
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, bytes).map_err(|e| e.to_string())?;
        Ok(Self { pair })
    }

    /// The public key browsers subscribe with (`applicationServerKey`), base64url.
    #[must_use]
    pub fn public_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.pair.public_key().as_ref())
    }

    /// The `Authorization` header for a push service at `endpoint`.
    ///
    /// # Errors
    ///
    /// When the endpoint isn't a URL or signing fails.
    pub fn authorization(&self, endpoint: &str, now: u64) -> Result<String, String> {
        let url = url::Url::parse(endpoint).map_err(|e| e.to_string())?;
        let audience = url.origin().ascii_serialization();
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({
            "aud": audience,
            "exp": now + 12 * 60 * 60,
            "sub": "https://github.com/PndaMan/delune",
        });
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signing_input = format!("{header}.{claims}");
        let signature = self.pair.sign(&SystemRandom::new(), signing_input.as_bytes()).map_err(|e| e.to_string())?;
        let token = format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()));
        Ok(format!("vapid t={token}, k={}", self.public_key()))
    }
}

/// What a browser gave us when it subscribed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's P-256 public key, base64url.
    pub p256dh: String,
    /// 16 random bytes shared with the browser, base64url.
    pub auth: String,
}

fn decode(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value.trim_end_matches('='))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(value))
        .map_err(|e| e.to_string())
}

fn hkdf_block(prk: &[u8], info: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, prk);
    let mut input = info.to_vec();
    input.push(1);
    hmac::sign(&key, &input).as_ref().to_vec()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data).as_ref().to_vec()
}

/// Derive the content key and nonce (RFC 8291 section 3.4 and RFC 8188).
fn derive(ecdh: &[u8], auth: &[u8], ua_public: &[u8], as_public: &[u8], salt: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let prk_key = hmac_sha256(auth, ecdh);
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let ikm = hkdf_block(&prk_key, &key_info);
    let prk = hmac_sha256(salt, &ikm);
    let cek = hkdf_block(&prk, b"Content-Encoding: aes128gcm\0")[..16].to_vec();
    let nonce = hkdf_block(&prk, b"Content-Encoding: nonce\0")[..12].to_vec();
    (cek, nonce)
}

/// Encrypt `payload` for `subscription`: the body of the push request.
///
/// # Errors
///
/// When the subscription's keys are malformed.
pub fn encrypt(subscription: &Subscription, payload: &[u8]) -> Result<Vec<u8>, String> {
    let ua_public = decode(&subscription.p256dh)?;
    let auth = decode(&subscription.auth)?;
    if ua_public.len() != 65 || auth.len() < 16 {
        return Err("the subscription's keys have the wrong length".into());
    }
    let rng = SystemRandom::new();
    let mut salt = [0u8; 16];
    rng.fill(&mut salt).map_err(|e| e.to_string())?;
    let private = EphemeralPrivateKey::generate(&ECDH_P256, &rng).map_err(|e| e.to_string())?;
    let as_public = private.compute_public_key().map_err(|e| e.to_string())?.as_ref().to_vec();
    let peer = UnparsedPublicKey::new(&ECDH_P256, &ua_public);
    let ecdh = agree_ephemeral(private, peer, aws_lc_rs::error::Unspecified, |secret| Ok(secret.to_vec()))
        .map_err(|_| "the browser's key isn't a valid P-256 point".to_owned())?;
    let (cek, nonce) = derive(&ecdh, &auth, &ua_public, &as_public, &salt);

    let mut record = payload.to_vec();
    record.push(2); // last (and only) record, no padding
    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).map_err(|e| e.to_string())?);
    let nonce = Nonce::try_assume_unique_for_key(&nonce).map_err(|e| e.to_string())?;
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut record).map_err(|e| e.to_string())?;

    let mut body = Vec::with_capacity(16 + 4 + 1 + as_public.len() + record.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(as_public.len()).unwrap_or(65));
    body.extend_from_slice(&as_public);
    body.extend_from_slice(&record);
    Ok(body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// The subscription has ended; forget it.
    Gone,
    Failed,
}

/// Send `payload` (JSON the service worker shows) to one subscription.
pub async fn send(http: &reqwest::Client, vapid: &Vapid, subscription: &Subscription, payload: &[u8]) -> Outcome {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (Ok(body), Ok(authorization)) =
        (encrypt(subscription, payload), vapid.authorization(&subscription.endpoint, now))
    else {
        return Outcome::Failed;
    };
    let response = http
        .post(&subscription.endpoint)
        .header("authorization", authorization)
        .header("content-encoding", "aes128gcm")
        .header("content-type", "application/octet-stream")
        .header("ttl", "86400")
        .header("urgency", "normal")
        .body(body)
        .send()
        .await;
    match response {
        Ok(r) if r.status().is_success() => Outcome::Delivered,
        Ok(r) if matches!(r.status().as_u16(), 404 | 410) => Outcome::Gone,
        Ok(r) => {
            tracing::debug!(status = %r.status(), "push service refused a notification");
            Outcome::Failed
        }
        Err(error) => {
            tracing::debug!(%error, "couldn't reach a push service");
            Outcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::agreement::PrivateKey;

    use super::*;

    #[test]
    fn a_browser_can_read_what_delune_encrypts() {
        // The browser's side: a key pair and an auth secret.
        let browser = PrivateKey::generate(&ECDH_P256).unwrap();
        let browser_public = browser.compute_public_key().unwrap().as_ref().to_vec();
        let auth = [7u8; 16];
        let subscription = Subscription {
            endpoint: "https://push.example.com/abc".into(),
            p256dh: URL_SAFE_NO_PAD.encode(&browser_public),
            auth: URL_SAFE_NO_PAD.encode(auth),
        };
        let message = br#"{"title":"USB is ready for review"}"#;
        let body = encrypt(&subscription, message).unwrap();

        // Decrypt as the browser would.
        let (salt, rest) = body.split_at(16);
        assert_eq!(u32::from_be_bytes(rest[..4].try_into().unwrap()), RECORD_SIZE);
        let key_len = rest[4] as usize;
        let sender_public = &rest[5..5 + key_len];
        let ciphertext = &rest[5 + key_len..];
        let ecdh = aws_lc_rs::agreement::agree(
            &browser,
            UnparsedPublicKey::new(&ECDH_P256, sender_public),
            aws_lc_rs::error::Unspecified,
            |secret| Ok(secret.to_vec()),
        )
        .unwrap();
        let (cek, nonce) = derive(&ecdh, &auth, &browser_public, sender_public, salt);
        let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).unwrap());
        let mut data = ciphertext.to_vec();
        let plain =
            key.open_in_place(Nonce::try_assume_unique_for_key(&nonce).unwrap(), Aad::empty(), &mut data).unwrap();
        assert_eq!(plain.last(), Some(&2), "one final record");
        assert_eq!(&plain[..plain.len() - 1], message);
    }

    #[test]
    fn signs_for_the_push_service() {
        let (vapid, pkcs8) = Vapid::generate().unwrap();
        let again = Vapid::from_pkcs8(&pkcs8).unwrap();
        assert_eq!(vapid.public_key(), again.public_key(), "the saved key loads back");
        assert_eq!(URL_SAFE_NO_PAD.decode(vapid.public_key()).unwrap().len(), 65);

        let header = vapid.authorization("https://fcm.googleapis.com/fcm/send/xyz", 1_000).unwrap();
        let token = header.strip_prefix("vapid t=").unwrap().split(", k=").next().unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 1_000 + 12 * 60 * 60);

        let public = aws_lc_rs::signature::UnparsedPublicKey::new(
            &aws_lc_rs::signature::ECDSA_P256_SHA256_FIXED,
            URL_SAFE_NO_PAD.decode(vapid.public_key()).unwrap(),
        );
        let signed = format!("{}.{}", parts[0], parts[1]);
        public.verify(signed.as_bytes(), &URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    }
}
