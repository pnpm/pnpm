use super::{
    ArtifactPayload, ArtifactProtocolError, BASE64, MAX_ENCODED_SIGNATURE_SIZE,
    MAX_ENCODED_SIGNED_PAYLOAD_SIZE, MAX_SIGNED_PAYLOAD_SIZE, SIGNATURE_ALGORITHM, Sha256,
    Signature, SignedArtifactEnvelope, SigningKey, VerifyingKey, hex, validate_scalar,
};
use base64::Engine as _;
use p256::{
    ecdsa::signature::{Signer as _, Verifier as _},
    pkcs8::{DecodePrivateKey as _, DecodePublicKey as _},
};
use sha2::Digest as _;

fn decode_payload_json(payload_bytes: &[u8]) -> Result<ArtifactPayload, ArtifactProtocolError> {
    serde_json::from_slice(payload_bytes).map_err(|error| {
        ArtifactProtocolError::InvalidEnvelope(format!("payload is not valid JSON: {error}"))
    })
}

impl SignedArtifactEnvelope {
    pub fn sign(
        payload: &ArtifactPayload,
        key_id: impl Into<String>,
        private_key_pkcs8: &[u8],
    ) -> Result<Self, ArtifactProtocolError> {
        payload.validate()?;
        let key_id = key_id.into();
        validate_scalar("key id", &key_id, 256)?;
        let payload_bytes = serde_json::to_vec(payload).map_err(|error| {
            ArtifactProtocolError::InvalidEnvelope(format!(
                "payload could not be serialized: {error}",
            ))
        })?;
        if payload_bytes.len() > MAX_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        let private_key = SigningKey::from_pkcs8_der(private_key_pkcs8).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope(
                "private key is not PKCS#8-encoded P-256 key material".to_string(),
            )
        })?;
        let signature: Signature = private_key.sign(&payload_bytes);
        Ok(Self {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id,
            payload: BASE64.encode(payload_bytes),
            signature: BASE64.encode(signature.to_der().as_bytes()),
        })
    }

    pub fn decode_payload(&self) -> Result<(ArtifactPayload, Vec<u8>), ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let payload = decode_payload_json(&payload_bytes)?;
        payload.validate()?;
        Ok((payload, payload_bytes))
    }

    pub(super) fn decode_payload_bytes(&self) -> Result<Vec<u8>, ArtifactProtocolError> {
        validate_scalar("signature algorithm", &self.algorithm, 64)?;
        if self.algorithm != SIGNATURE_ALGORITHM {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "unsupported signature algorithm {:?}",
                self.algorithm,
            )));
        }
        validate_scalar("key id", &self.key_id, 256)?;
        if self.payload.len() > MAX_ENCODED_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        let payload_bytes = BASE64.decode(&self.payload).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope("payload is not valid base64".to_string())
        })?;
        if BASE64.encode(&payload_bytes) != self.payload {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "payload is not canonical base64".to_string(),
            ));
        }
        if payload_bytes.len() > MAX_SIGNED_PAYLOAD_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(format!(
                "signed payload exceeds {MAX_SIGNED_PAYLOAD_SIZE} bytes",
            )));
        }
        Ok(payload_bytes)
    }

    pub fn verify(&self, public_key_spki: &[u8]) -> Result<ArtifactPayload, ArtifactProtocolError> {
        let payload = self.verify_signature(public_key_spki)?;
        payload.validate()?;
        Ok(payload)
    }

    pub fn verify_signature(
        &self,
        public_key_spki: &[u8],
    ) -> Result<ArtifactPayload, ArtifactProtocolError> {
        let payload_bytes = self.verify_signature_bytes(public_key_spki)?;
        decode_payload_json(&payload_bytes)
    }

    pub fn verify_signature_bytes(
        &self,
        public_key_spki: &[u8],
    ) -> Result<Vec<u8>, ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let (signature, _) = self.decode_signature()?;
        let public_key = VerifyingKey::from_public_key_der(public_key_spki)
            .map_err(|_| ArtifactProtocolError::InvalidSignature)?;
        public_key
            .verify(&payload_bytes, &signature)
            .map_err(|_| ArtifactProtocolError::InvalidSignature)?;
        Ok(payload_bytes)
    }

    pub fn digest(&self) -> Result<String, ArtifactProtocolError> {
        let payload_bytes = self.decode_payload_bytes()?;
        let (_, signature_bytes) = self.decode_signature()?;
        let mut hasher = Sha256::new();
        hasher.update(b"pnpm-shared-artifact-envelope-v1\0");
        hasher.update(self.algorithm.as_bytes());
        hasher.update([0]);
        hasher.update(self.key_id.as_bytes());
        hasher.update([0]);
        hasher.update(payload_bytes);
        hasher.update([0]);
        hasher.update(signature_bytes);
        Ok(hex(&hasher.finalize()))
    }

    pub(super) fn decode_signature(&self) -> Result<(Signature, Vec<u8>), ArtifactProtocolError> {
        if self.signature.len() > MAX_ENCODED_SIGNATURE_SIZE {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not a DER-encoded P-256 signature".to_string(),
            ));
        }
        let signature_bytes = BASE64.decode(&self.signature).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope("signature is not valid base64".to_string())
        })?;
        if BASE64.encode(&signature_bytes) != self.signature {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not canonical base64".to_string(),
            ));
        }
        let signature = Signature::from_der(&signature_bytes).map_err(|_| {
            ArtifactProtocolError::InvalidEnvelope(
                "signature is not a DER-encoded P-256 signature".to_string(),
            )
        })?;
        if signature.to_der().as_bytes() != signature_bytes {
            return Err(ArtifactProtocolError::InvalidEnvelope(
                "signature is not canonical DER".to_string(),
            ));
        }
        Ok((signature, signature_bytes))
    }
}
