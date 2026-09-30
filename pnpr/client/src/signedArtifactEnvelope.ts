import { createHash, createPrivateKey, createPublicKey, KeyObject, sign as cryptoSign, verify as cryptoVerify } from 'node:crypto'
import { TextDecoder } from 'node:util'

import { decodeBase64, validateScalar } from './artifactEncoding.js'
import { compatibilityRank } from './compatibilityTags.js'
import {
  type ArtifactPayload,
  type CreateSignedArtifactEnvelopeOptions,
  MAX_BASE64_SIGNATURE_LENGTH,
  MAX_BASE64_SIGNED_PAYLOAD_LENGTH,
  MAX_SIGNED_PAYLOAD_SIZE,
  type OwnerScope,
  SIGNATURE_ALGORITHM,
  type SignedArtifactEnvelope,
  type VerifiedArtifact,
  type VerifyStoredSharedSideEffectsOptions,
} from './sharedArtifactTypes.js'
import { ownersEqual, subjectsEqual, validatePayload } from './sharedArtifactValidation.js'

export function createSignedArtifactEnvelope (
  payload: ArtifactPayload,
  opts: CreateSignedArtifactEnvelopeOptions
): SignedArtifactEnvelope {
  validatePayload(payload)
  validateScalar('key id', opts.keyId, 256)
  const payloadBytes = Buffer.from(JSON.stringify(payload))
  if (payloadBytes.byteLength > MAX_SIGNED_PAYLOAD_SIZE) {
    throw new Error(`Signed artifact payload exceeds ${MAX_SIGNED_PAYLOAD_SIZE} bytes`)
  }
  const privateKey = opts.privateKey instanceof KeyObject
    ? opts.privateKey
    : createPrivateKey(opts.privateKey)
  if (privateKey.asymmetricKeyType !== 'ec' || privateKey.asymmetricKeyDetails?.namedCurve !== 'prime256v1') {
    throw new Error('Shared artifact signing key must be a P-256 EC private key')
  }
  const signature = cryptoSign('sha256', payloadBytes, {
    key: privateKey,
    dsaEncoding: 'der',
  })
  return {
    algorithm: SIGNATURE_ALGORITHM,
    keyId: opts.keyId,
    payload: payloadBytes.toString('base64'),
    signature: signature.toString('base64'),
  }
}

export function ownerNamespace (owner: OwnerScope): string {
  return owner.type === 'organization'
    ? `organization:${owner.name}`
    : `publisher:${owner.package}`
}

export function verifyStoredSharedSideEffects (
  opts: VerifyStoredSharedSideEffectsOptions
): VerifiedArtifact {
  const payload = verifySignedArtifactEnvelope(opts.envelope, opts.publicKey)
  if (
    payload.inputKey !== opts.candidate.key ||
    !subjectsEqual(payload.subject, opts.candidate.subject) ||
    !ownersEqual(payload.owner, opts.candidate.owner) ||
    compatibilityRank(payload.compatibility, opts.supportedTags) == null
  ) {
    throw new Error('Stored shared artifact no longer matches the package or consumer')
  }
  const envelopeDigest = signedArtifactEnvelopeDigest(opts.envelope)
  return { payload, envelope: opts.envelope, envelopeDigest }
}

export function verifySignedArtifactEnvelope (
  envelope: SignedArtifactEnvelope,
  publicKeySpki: string
): ArtifactPayload {
  const decoded = decodeEnvelopeFields(envelope)
  verifyEnvelopeSignature(decoded, publicKeySpki)
  const payload = decodeArtifactPayload(decoded.payloadBytes)
  validatePayload(payload)
  return payload
}

export function verifyEnvelopeSignature (
  { payloadBytes, signatureBytes }: DecodedEnvelopeFields,
  publicKeySpki: string
): void {
  const publicKey = createPublicKey({
    key: Buffer.from(publicKeySpki, 'base64'),
    format: 'der',
    type: 'spki',
  })
  if (publicKey.asymmetricKeyType !== 'ec' || publicKey.asymmetricKeyDetails?.namedCurve !== 'prime256v1') {
    throw new Error('Shared artifact verification key must be a P-256 EC public key')
  }
  if (!cryptoVerify('sha256', payloadBytes, publicKey, signatureBytes)) {
    throw new Error('Shared artifact signature verification failed')
  }
}

export interface DecodedEnvelopeFields {
  payloadBytes: Buffer
  signatureBytes: Buffer
}

export function decodeEnvelopeFields (envelope: SignedArtifactEnvelope): DecodedEnvelopeFields {
  if (envelope.algorithm !== SIGNATURE_ALGORITHM) {
    throw new Error(`Unsupported shared artifact signature algorithm ${JSON.stringify(envelope.algorithm)}`)
  }
  validateScalar('key id', envelope.keyId, 256)
  if (typeof envelope.payload !== 'string' || envelope.payload.length > MAX_BASE64_SIGNED_PAYLOAD_LENGTH) {
    throw new Error(`Signed artifact payload exceeds ${MAX_SIGNED_PAYLOAD_SIZE} bytes`)
  }
  const payloadBytes = decodeBase64('signed payload', envelope.payload)
  if (payloadBytes.byteLength > MAX_SIGNED_PAYLOAD_SIZE) {
    throw new Error(`Signed artifact payload exceeds ${MAX_SIGNED_PAYLOAD_SIZE} bytes`)
  }
  if (typeof envelope.signature !== 'string' || envelope.signature.length > MAX_BASE64_SIGNATURE_LENGTH) {
    throw new Error('Shared artifact signature is not canonical P-256 DER')
  }
  const signatureBytes = decodeBase64('signature', envelope.signature)
  validateP256DerSignature(signatureBytes)
  return { payloadBytes, signatureBytes }
}

export function decodeEnvelope (envelope: SignedArtifactEnvelope): DecodedEnvelopeFields & { payload: ArtifactPayload } {
  const decoded = decodeEnvelopeFields(envelope)
  const payload = decodeArtifactPayload(decoded.payloadBytes)
  validatePayload(payload)
  return { ...decoded, payload }
}

export function decodeArtifactPayload (payloadBytes: Buffer): ArtifactPayload {
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(payloadBytes)) as ArtifactPayload
}

export function signedArtifactEnvelopeDigest (envelope: SignedArtifactEnvelope): string {
  return digestDecodedEnvelope(envelope, decodeEnvelopeFields(envelope))
}

export function digestDecodedEnvelope (
  envelope: SignedArtifactEnvelope,
  { payloadBytes, signatureBytes }: DecodedEnvelopeFields
): string {
  return createHash('sha256')
    .update('pnpm-shared-artifact-envelope-v1\0')
    .update(envelope.algorithm)
    .update('\0')
    .update(envelope.keyId)
    .update('\0')
    .update(payloadBytes)
    .update('\0')
    .update(signatureBytes)
    .digest('hex')
}

function validateP256DerSignature (signature: Buffer): void {
  if (signature.length < 8 || signature[0] !== 0x30 || signature[1] !== signature.length - 2) {
    throw new Error('Shared artifact signature is not canonical P-256 DER')
  }
  let offset = 2
  for (let integer = 0; integer < 2; integer++) {
    if (signature[offset] !== 0x02) throw new Error('Shared artifact signature is not canonical P-256 DER')
    const length = signature[offset + 1]
    const start = offset + 2
    const end = start + length
    const isNegative = (signature[start] & 0x80) !== 0
    const hasRedundantLeadingZero = length > 1 && signature[start] === 0 && (signature[start + 1] & 0x80) === 0
    if (length === 0 || length > 33 || end > signature.length || isNegative || hasRedundantLeadingZero) {
      throw new Error('Shared artifact signature is not canonical P-256 DER')
    }
    offset = end
  }
  if (offset !== signature.length) throw new Error('Shared artifact signature is not canonical P-256 DER')
}
