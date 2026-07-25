use serde::{Deserialize, Serialize};

use crate::error::{AQBotError, Result};
use crate::types::BedrockCredentialInput;

const BEDROCK_CREDENTIAL_VERSION: u8 = 1;
const BEDROCK_CREDENTIAL_KIND: &str = "aws_sigv4";

#[derive(Serialize, Deserialize)]
struct StoredBedrockCredential {
    version: u8,
    kind: String,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

pub fn normalize(input: BedrockCredentialInput) -> Result<BedrockCredentialInput> {
    let access_key_id = input.access_key_id.trim().to_string();
    let secret_access_key = input.secret_access_key.trim().to_string();
    let session_token = input
        .session_token
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if access_key_id.is_empty() {
        return Err(AQBotError::Validation(
            "AWS access key ID is required".into(),
        ));
    }
    if secret_access_key.is_empty() {
        return Err(AQBotError::Validation(
            "AWS secret access key is required".into(),
        ));
    }

    Ok(BedrockCredentialInput {
        access_key_id,
        secret_access_key,
        session_token,
    })
}

pub fn serialize(input: &BedrockCredentialInput) -> Result<String> {
    let stored = StoredBedrockCredential {
        version: BEDROCK_CREDENTIAL_VERSION,
        kind: BEDROCK_CREDENTIAL_KIND.into(),
        access_key_id: input.access_key_id.clone(),
        secret_access_key: input.secret_access_key.clone(),
        session_token: input.session_token.clone(),
    };
    serde_json::to_string(&stored)
        .map_err(|_| AQBotError::Validation("Failed to encode AWS credentials".into()))
}

pub fn parse(value: &str) -> Result<BedrockCredentialInput> {
    let stored: StoredBedrockCredential = serde_json::from_str(value)
        .map_err(|_| AQBotError::Validation("Invalid AWS credential data".into()))?;
    if stored.version != BEDROCK_CREDENTIAL_VERSION || stored.kind != BEDROCK_CREDENTIAL_KIND {
        return Err(AQBotError::Validation(
            "Unsupported AWS credential data version".into(),
        ));
    }
    normalize(BedrockCredentialInput {
        access_key_id: stored.access_key_id,
        secret_access_key: stored.secret_access_key,
        session_token: stored.session_token,
    })
}

pub fn key_prefix(access_key_id: &str) -> String {
    let prefix: String = access_key_id.chars().take(8).collect();
    if access_key_id.chars().count() > 8 {
        format!("{prefix}...")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_round_trip_without_debug_output() {
        let input = normalize(BedrockCredentialInput {
            access_key_id: " AKIAEXAMPLE ".into(),
            secret_access_key: " secret ".into(),
            session_token: Some(" token ".into()),
        })
        .unwrap();
        let stored = serialize(&input).unwrap();
        let parsed = parse(&stored).unwrap();

        assert_eq!(parsed.access_key_id, input.access_key_id);
        assert_eq!(parsed.secret_access_key, input.secret_access_key);
        assert_eq!(parsed.session_token, input.session_token);
        assert_eq!(key_prefix(&parsed.access_key_id), "AKIAEXAM...");
    }

    #[test]
    fn rejects_invalid_or_incomplete_credentials() {
        assert!(normalize(BedrockCredentialInput {
            access_key_id: String::new(),
            secret_access_key: "secret".into(),
            session_token: None,
        })
        .is_err());
        assert!(parse(r#"{"version":2,"kind":"aws_sigv4"}"#).is_err());
        assert!(parse("not-json").is_err());
    }

    #[test]
    fn encrypted_envelope_does_not_expose_secret_or_session_token() {
        let input = BedrockCredentialInput {
            access_key_id: "AKIAEXAMPLE".into(),
            secret_access_key: "secret-value-not-plaintext".into(),
            session_token: Some("session-value-not-plaintext".into()),
        };
        let envelope = serialize(&input).unwrap();
        let encrypted = crate::crypto::encrypt_key(&envelope, &[42u8; 32]).unwrap();

        assert!(!encrypted.contains(&input.secret_access_key));
        assert!(!encrypted.contains(input.session_token.as_deref().unwrap()));

        let decrypted = crate::crypto::decrypt_key(&encrypted, &[42u8; 32]).unwrap();
        let parsed = parse(&decrypted).unwrap();
        assert_eq!(parsed.access_key_id, input.access_key_id);
        assert_eq!(parsed.secret_access_key, input.secret_access_key);
        assert_eq!(parsed.session_token, input.session_token);
    }

    #[test]
    fn parse_errors_do_not_echo_corrupted_credential_data() {
        let corrupted = r#"{"version":1,"kind":"aws_sigv4","secret_access_key":"do-not-leak"}"#;
        let error = match parse(corrupted) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("corrupted credentials were unexpectedly accepted"),
        };

        assert!(!error.contains("do-not-leak"));
    }
}
