use super::catalog::CatalogV1;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CATALOG_SIGNATURE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSignatureV1 {
    pub version: u32,
    pub algorithm: CatalogSignatureAlgorithm,
    pub key_id: String,
    pub catalog_sha256: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogSignatureAlgorithm {
    Ed25519,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCatalogKey {
    pub key_id: String,
    pub public_key: [u8; 32],
}

/// Public key for the user-approved, non-commercial static-review asset
/// release. The corresponding private key is intentionally kept outside the
/// repository and application build. Trusting this key verifies artifact
/// integrity; it does not upgrade any profile to real-platform verification.
pub fn trusted_catalog_keys() -> Vec<TrustedCatalogKey> {
    vec![TrustedCatalogKey {
        key_id: "device-assets-static-review-2026".into(),
        public_key: [
            13, 90, 199, 12, 72, 36, 215, 99, 25, 160, 143, 21, 237, 43, 45, 235, 57, 13, 176, 92,
            75, 31, 158, 90, 186, 220, 52, 215, 129, 142, 235, 57,
        ],
    }]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSignatureError {
    pub code: &'static str,
    pub message: String,
}

impl CatalogSignatureError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CatalogSignatureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CatalogSignatureError {}

/// Verifies the detached signature over the catalog's original bytes before
/// deserializing any untrusted catalog content.
pub fn verify_signed_catalog(
    catalog_bytes: &[u8],
    signature_bytes: &[u8],
    trusted_keys: &[TrustedCatalogKey],
) -> Result<CatalogV1, CatalogSignatureError> {
    let envelope: CatalogSignatureV1 =
        serde_json::from_slice(signature_bytes).map_err(|error| {
            CatalogSignatureError::new(
                "device_simulator.assets.catalog_signature_invalid",
                format!("invalid catalog signature envelope: {error}"),
            )
        })?;

    if envelope.version != CATALOG_SIGNATURE_VERSION {
        return Err(CatalogSignatureError::new(
            "device_simulator.assets.catalog_signature_version_unsupported",
            format!("unsupported catalog signature version {}", envelope.version),
        ));
    }

    let actual_sha256 = lowercase_hex(&Sha256::digest(catalog_bytes));
    if !is_lowercase_sha256(&envelope.catalog_sha256) || envelope.catalog_sha256 != actual_sha256 {
        return Err(CatalogSignatureError::new(
            "device_simulator.assets.catalog_signature_hash_mismatch",
            "catalog bytes do not match the signed SHA-256 digest",
        ));
    }

    let trusted_key = trusted_keys
        .iter()
        .find(|candidate| candidate.key_id == envelope.key_id)
        .ok_or_else(|| {
            CatalogSignatureError::new(
                "device_simulator.assets.catalog_signature_key_unknown",
                format!("catalog signature key '{}' is not trusted", envelope.key_id),
            )
        })?;
    let verifying_key = VerifyingKey::from_bytes(&trusted_key.public_key).map_err(|error| {
        CatalogSignatureError::new(
            "device_simulator.assets.catalog_signature_key_invalid",
            format!("trusted catalog public key is invalid: {error}"),
        )
    })?;
    let signature_raw = BASE64_STANDARD
        .decode(envelope.signature.as_bytes())
        .map_err(|error| {
            CatalogSignatureError::new(
                "device_simulator.assets.catalog_signature_invalid",
                format!("catalog signature is not valid base64: {error}"),
            )
        })?;
    let signature = Signature::from_slice(&signature_raw).map_err(|error| {
        CatalogSignatureError::new(
            "device_simulator.assets.catalog_signature_invalid",
            format!("catalog signature has an invalid length: {error}"),
        )
    })?;

    verifying_key
        .verify(catalog_bytes, &signature)
        .map_err(|_| {
            CatalogSignatureError::new(
                "device_simulator.assets.catalog_signature_verification_failed",
                "catalog signature verification failed",
            )
        })?;

    serde_json::from_slice(catalog_bytes).map_err(|error| {
        CatalogSignatureError::new(
            "device_simulator.assets.catalog_invalid",
            format!("signed catalog JSON is invalid: {error}"),
        )
    })
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn signed_fixture() -> (Vec<u8>, Vec<u8>, TrustedCatalogKey) {
        let catalog = serde_json::to_vec(&json!({
            "schema_version": 1,
            "generated_at": "2026-07-18T12:00:00+08:00",
            "engine_api": 1,
            "packs": [],
            "profiles": []
        }))
        .unwrap();
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let signature = signing_key.sign(&catalog);
        let envelope = serde_json::to_vec(&CatalogSignatureV1 {
            version: CATALOG_SIGNATURE_VERSION,
            algorithm: CatalogSignatureAlgorithm::Ed25519,
            key_id: "assets-test-1".into(),
            catalog_sha256: lowercase_hex(&Sha256::digest(&catalog)),
            signature: BASE64_STANDARD.encode(signature.to_bytes()),
        })
        .unwrap();
        let trusted_key = TrustedCatalogKey {
            key_id: "assets-test-1".into(),
            public_key: signing_key.verifying_key().to_bytes(),
        };
        (catalog, envelope, trusted_key)
    }

    #[test]
    fn verifies_original_bytes_before_parsing_catalog() {
        let (catalog, envelope, trusted_key) = signed_fixture();
        let parsed = verify_signed_catalog(&catalog, &envelope, &[trusted_key]).unwrap();
        assert_eq!(parsed.schema_version, 1);
    }

    #[test]
    fn rejects_tampering_unknown_keys_and_missing_signatures() {
        let (catalog, envelope, trusted_key) = signed_fixture();

        let mut tampered = catalog.clone();
        tampered.push(b' ');
        assert_eq!(
            verify_signed_catalog(&tampered, &envelope, &[trusted_key.clone()])
                .unwrap_err()
                .code,
            "device_simulator.assets.catalog_signature_hash_mismatch"
        );
        assert_eq!(
            verify_signed_catalog(&catalog, &envelope, &[])
                .unwrap_err()
                .code,
            "device_simulator.assets.catalog_signature_key_unknown"
        );
        assert_eq!(
            verify_signed_catalog(&catalog, b"", &[trusted_key])
                .unwrap_err()
                .code,
            "device_simulator.assets.catalog_signature_invalid"
        );
    }

    #[test]
    fn rejects_a_valid_hash_with_a_signature_for_different_bytes() {
        let (catalog, envelope, _) = signed_fixture();
        let other_key = SigningKey::from_bytes(&[9_u8; 32]);
        let trusted_key = TrustedCatalogKey {
            key_id: "assets-test-1".into(),
            public_key: other_key.verifying_key().to_bytes(),
        };

        assert_eq!(
            verify_signed_catalog(&catalog, &envelope, &[trusted_key])
                .unwrap_err()
                .code,
            "device_simulator.assets.catalog_signature_verification_failed"
        );
    }
}
