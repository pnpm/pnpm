import { artifactBlobDigest, decodeBase64, validateScalar, verifyBlob } from './artifactEncoding.js'
import { rankCompatibility, validateSupportedTags } from './compatibilityTags.js'
import { errorMessage } from './errorMessage.js'
import { assertSuccess, request } from './pnprArtifactRequest.js'
import {
  type ArtifactBlobRequest,
  type ArtifactBlobUpload,
  type ArtifactPayload,
  type ArtifactVariant,
  type DependencySideEffectsCandidate,
  MAX_ARTIFACT_SIZE,
  MAX_BASE64_BLOB_LENGTH,
  MAX_CANDIDATES,
  MAX_FILE_SIZE,
  MAX_LOOKUP_RESPONSE_SIZE,
  MAX_PUBLISH_REQUEST_SIZE,
  MAX_VARIANTS_PER_CANDIDATE,
  type PublishSharedSideEffectsOptions,
  type ResolveArtifactsResponse,
  type ResolveSharedSideEffectsOptions,
  type VerifiedArtifact,
} from './sharedArtifactTypes.js'
import { ownersEqual, subjectArtifactIdentity, subjectsEqual, validateCandidate, validateOwner, validatePayload } from './sharedArtifactValidation.js'
import {
  decodeArtifactPayload,
  type DecodedEnvelopeFields,
  decodeEnvelope,
  decodeEnvelopeFields,
  digestDecodedEnvelope,
  verifyEnvelopeSignature,
} from './signedArtifactEnvelope.js'

export { artifactBlobDigest } from './artifactEncoding.js'
export {
  compatibilityRank,
  linuxGlibcCompatibilityTag,
  linuxGlibcSupportedTags,
  macOSCompatibilityTag,
  macOSSupportedTags,
  platformFingerprint,
  windowsCompatibilityTag,
  windowsSupportedTags,
} from './compatibilityTags.js'
export {
  ARTIFACT_KIND,
  type ArtifactBlobRequest,
  type ArtifactBlobUpload,
  type ArtifactCandidate,
  type ArtifactFile,
  type ArtifactManifest,
  type ArtifactPayload,
  type ArtifactSubject,
  type BuilderProfile,
  COMPATIBILITY_TAG_SCHEMA,
  type CompatibilityConstraints,
  type CreateSignedArtifactEnvelopeOptions,
  DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND,
  DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX,
  type DependencySideEffectsCandidate,
  type DependencySideEffectsPayload,
  type DependencySideEffectsSubject,
  INPUT_KEY_PREFIX,
  type LinuxGlibcPlatform,
  type MacOSPlatform,
  type OwnerScope,
  type PackageIdentity,
  type PublishSharedSideEffectsOptions,
  type ResolveSharedSideEffectsOptions,
  SIGNATURE_ALGORITHM,
  type SignedArtifactEnvelope,
  type VerifiedArtifact,
  type VerifyStoredSharedSideEffectsOptions,
  type WindowsPlatform,
  WORKSPACE_TASK_ARTIFACT_KIND,
  WORKSPACE_TASK_INPUT_KEY_PREFIX,
  type WorkspaceTaskPayload,
  type WorkspaceTaskSubject,
} from './sharedArtifactTypes.js'
export {
  createSignedArtifactEnvelope,
  ownerNamespace,
  signedArtifactEnvelopeDigest,
  verifySignedArtifactEnvelope,
  verifyStoredSharedSideEffects,
} from './signedArtifactEnvelope.js'

interface RankedArtifact {
  rank: number
  artifact: VerifiedArtifact
}

interface BlobUploadTotals {
  uploadedSize: number
  encodedSize: number
}

export async function publishSharedSideEffects (
  opts: PublishSharedSideEffectsOptions
): Promise<void> {
  const response = await request({
    registryUrl: opts.registryUrl,
    path: '-/pnpr/v0/artifacts',
    method: 'PUT',
    authorization: opts.authorization,
    body: serializePublishRequest(opts),
    maxResponseSize: 64 * 1024,
  })
  assertSuccess(response, '/-/pnpr/v0/artifacts')
}

export async function pnprSupportsSharedSideEffects (
  opts: Pick<ResolveSharedSideEffectsOptions, 'registryUrl' | 'authorization'>
): Promise<boolean> {
  const response = await request({
    registryUrl: opts.registryUrl,
    path: '-/pnpr',
    method: 'GET',
    authorization: opts.authorization,
    maxResponseSize: 64 * 1024,
  })
  if (response.statusCode < 200 || response.statusCode >= 300) return false
  const parsed = JSON.parse(response.body.toString('utf8')) as {
    pnpr?: { artifacts?: unknown }
  }
  return Array.isArray(parsed.pnpr?.artifacts) && parsed.pnpr.artifacts.includes(0)
}

export async function resolveSharedSideEffects (
  opts: ResolveSharedSideEffectsOptions
): Promise<Map<string, VerifiedArtifact>> {
  validateSupportedTags(opts.supportedTags)
  if (opts.policy.ignoreScripts) return new Map()
  const permittedCandidates = opts.candidates.filter(candidate =>
    opts.policy.eligiblePackages.has(candidate.subject.package.name) && opts.policy.allowedBuilds.has(candidate.subject.package.name)
  )
  if (permittedCandidates.length === 0) return new Map()
  if (permittedCandidates.length > MAX_CANDIDATES) {
    throw new Error(`Shared artifact lookup exceeds the ${MAX_CANDIDATES}-candidate limit`)
  }
  const candidates = indexCandidatesByKey(permittedCandidates)
  const response = await request({
    registryUrl: opts.registryUrl,
    path: '-/pnpr/v0/artifacts/resolve',
    method: 'POST',
    authorization: opts.authorization,
    body: Buffer.from(JSON.stringify({ candidates: permittedCandidates })),
    maxResponseSize: MAX_LOOKUP_RESPONSE_SIZE,
  })
  assertSuccess(response, '/-/pnpr/v0/artifacts/resolve')
  const parsed = parseResolveResponse(response.body)
  if (parsed.artifacts.length > candidates.size) {
    throw new Error('Shared artifact response contains more entries than requested')
  }
  return selectResolvedArtifacts(opts, candidates, parsed.artifacts)
}

function indexCandidatesByKey (
  permittedCandidates: DependencySideEffectsCandidate[]
): Map<string, DependencySideEffectsCandidate> {
  const candidates = new Map<string, DependencySideEffectsCandidate>()
  for (const candidate of permittedCandidates) {
    validateCandidate(candidate)
    if (candidates.has(candidate.key)) {
      throw new Error(`Duplicate shared artifact candidate ${JSON.stringify(candidate.key)}`)
    }
    candidates.set(candidate.key, candidate)
  }
  return candidates
}

function selectResolvedArtifacts (
  opts: ResolveSharedSideEffectsOptions,
  candidates: Map<string, DependencySideEffectsCandidate>,
  artifacts: ResolveArtifactsResponse['artifacts']
): Map<string, VerifiedArtifact> {
  const selected = new Map<string, VerifiedArtifact>()
  const responseKeys = new Set<string>()
  for (const artifact of artifacts) {
    if (responseKeys.has(artifact.key)) {
      throw new Error(`Shared artifact response repeats key ${JSON.stringify(artifact.key)}`)
    }
    responseKeys.add(artifact.key)
    const candidate = candidates.get(artifact.key)
    if (candidate == null) {
      throw new Error(`Shared artifact response returned a key that was not requested: ${JSON.stringify(artifact.key)}`)
    }
    if (artifact.variants.length > MAX_VARIANTS_PER_CANDIDATE) {
      throw new Error(`Shared artifact response exceeds the per-key variant limit for ${JSON.stringify(artifact.key)}`)
    }
    const best = selectBestVariant(opts, candidate, artifact.variants)
    if (best != null) selected.set(candidate.key, best.artifact)
  }
  return selected
}

function selectBestVariant (
  opts: ResolveSharedSideEffectsOptions,
  candidate: DependencySideEffectsCandidate,
  variants: ArtifactVariant[]
): RankedArtifact | undefined {
  let best: RankedArtifact | undefined
  for (const variant of variants) {
    const ranked = rankVariant(opts, candidate, variant)
    if (ranked != null && outranks(ranked, best)) best = ranked
  }
  return best
}

function outranks (ranked: RankedArtifact, best: RankedArtifact | undefined): boolean {
  return best == null ||
    ranked.rank < best.rank ||
    (ranked.rank === best.rank && ranked.artifact.envelopeDigest < best.artifact.envelopeDigest)
}

function rankVariant (
  opts: ResolveSharedSideEffectsOptions,
  candidate: DependencySideEffectsCandidate,
  variant: ArtifactVariant
): RankedArtifact | undefined {
  const publicKey = opts.trustedKeys[variant.envelope.keyId]
  if (publicKey == null) return undefined
  const decoded = decodeSignedVariant(variant, publicKey)
  if (decoded == null) return undefined
  const digest = digestDecodedEnvelope(variant.envelope, decoded)
  if (opts.quarantinedEnvelopeDigests?.get(candidate.key)?.has(digest) === true) return undefined
  const payload = decodeVariantPayload(decoded, { opts, inputKey: candidate.key, envelopeDigest: digest })
  if (payload == null || !payloadMatchesCandidate(payload, candidate)) return undefined
  const rank = rankCompatibility(payload.compatibility, opts.supportedTags)
  if (rank == null) return undefined
  return {
    rank,
    artifact: {
      payload,
      envelope: variant.envelope,
      envelopeDigest: digest,
    },
  }
}

function decodeSignedVariant (variant: ArtifactVariant, publicKey: string): DecodedEnvelopeFields | undefined {
  try {
    const decoded = decodeEnvelopeFields(variant.envelope)
    verifyEnvelopeSignature(decoded, publicKey)
    return decoded
  } catch {
    return undefined
  }
}

function decodeVariantPayload (
  decoded: DecodedEnvelopeFields,
  rejection: {
    opts: Pick<ResolveSharedSideEffectsOptions, 'onRejectedArtifact'>
    inputKey: string
    envelopeDigest: string
  }
): ArtifactPayload | undefined {
  try {
    const payload = decodeArtifactPayload(decoded.payloadBytes)
    validatePayload(payload)
    return payload
  } catch (err: unknown) {
    rejection.opts.onRejectedArtifact?.({
      inputKey: rejection.inputKey,
      envelopeDigest: rejection.envelopeDigest,
      reason: errorMessage(err),
    })
    return undefined
  }
}

function payloadMatchesCandidate (payload: ArtifactPayload, candidate: DependencySideEffectsCandidate): boolean {
  return payload.inputKey === candidate.key &&
    subjectsEqual(payload.subject, candidate.subject) &&
    ownersEqual(payload.owner, candidate.owner)
}

export async function downloadSharedArtifactBlob (
  opts: {
    registryUrl: string
    authorization?: string
    request: ArtifactBlobRequest
  }
): Promise<Buffer> {
  validateOwner(opts.request.owner)
  artifactBlobDigest(opts.request.integrity)
  const response = await request({
    registryUrl: opts.registryUrl,
    path: '-/pnpr/v0/artifacts/blob',
    method: 'POST',
    authorization: opts.authorization,
    body: Buffer.from(JSON.stringify(opts.request)),
    maxResponseSize: MAX_FILE_SIZE,
  })
  assertSuccess(response, '/-/pnpr/v0/artifacts/blob')
  try {
    verifyBlob(opts.request.integrity, response.body)
  } catch (err: unknown) {
    throw new SharedArtifactBlobIntegrityError(errorMessage(err))
  }
  return response.body
}

export class SharedArtifactBlobIntegrityError extends Error {
  public readonly code = 'ERR_PNPM_SHARED_ARTIFACT_BLOB_INTEGRITY'
}

function serializePublishRequest (opts: PublishSharedSideEffectsOptions): Buffer {
  const payload = decodePublishedPayload(opts)
  if (!Array.isArray(opts.blobs)) throw new Error('Shared artifact blob uploads are malformed')

  const required = new Map(payload.manifest.added.map(file => [file.integrity, file.size]))
  const totals = validateBlobUploads(opts.blobs, required)
  const encodedSize = Buffer.byteLength(opts.key) +
    Buffer.byteLength(opts.envelope.keyId) +
    Buffer.byteLength(opts.envelope.payload) +
    Buffer.byteLength(opts.envelope.signature) +
    totals.encodedSize
  if (totals.uploadedSize > MAX_ARTIFACT_SIZE || encodedSize > MAX_PUBLISH_REQUEST_SIZE) {
    throw new Error('Shared artifact publication exceeds the request size limit')
  }
  const body = Buffer.from(JSON.stringify({
    key: opts.key,
    envelope: opts.envelope,
    blobs: opts.blobs,
  }))
  if (body.byteLength > MAX_PUBLISH_REQUEST_SIZE) {
    throw new Error('Shared artifact publication exceeds the request size limit')
  }
  return body
}

function decodePublishedPayload (opts: PublishSharedSideEffectsOptions): ArtifactPayload {
  const { payload } = decodeEnvelope(opts.envelope)
  const { inputKeyPrefix } = subjectArtifactIdentity(payload.subject)
  if (typeof opts.key !== 'string' || !opts.key.startsWith(inputKeyPrefix)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(inputKeyPrefix)}`)
  }
  validateScalar('input key', opts.key, 4_096)
  if (payload.inputKey !== opts.key) {
    throw new Error('Signed shared artifact input key does not match the publication key')
  }
  return payload
}

function validateBlobUploads (blobs: ArtifactBlobUpload[], required: Map<string, number>): BlobUploadTotals {
  const uploads = new Set<string>()
  const totals: BlobUploadTotals = { uploadedSize: 0, encodedSize: 0 }
  for (const blob of blobs) {
    totals.uploadedSize += validateBlobUpload(blob, { required, uploads })
    totals.encodedSize += Buffer.byteLength(blob.integrity) + Buffer.byteLength(blob.data)
  }
  return totals
}

function validateBlobUpload (
  blob: ArtifactBlobUpload,
  { required, uploads }: { required: Map<string, number>, uploads: Set<string> }
): number {
  if (blob == null || typeof blob !== 'object') throw new Error('Shared artifact blob upload is malformed')
  artifactBlobDigest(blob.integrity)
  if (uploads.has(blob.integrity)) {
    throw new Error(`Duplicate shared artifact blob upload ${JSON.stringify(blob.integrity)}`)
  }
  uploads.add(blob.integrity)
  const expectedSize = required.get(blob.integrity)
  if (expectedSize == null) {
    throw new Error(`Shared artifact blob upload ${JSON.stringify(blob.integrity)} is not referenced by the signed manifest`)
  }
  if (typeof blob.data !== 'string' || blob.data.length > MAX_BASE64_BLOB_LENGTH) {
    throw new Error(`Shared artifact blob ${JSON.stringify(blob.integrity)} exceeds the encoded size limit`)
  }
  const bytes = decodeBase64('blob data', blob.data, true)
  if (bytes.byteLength !== expectedSize) {
    throw new Error(`Shared artifact blob has ${bytes.byteLength} bytes but the signed manifest declares ${expectedSize}`)
  }
  verifyBlob(blob.integrity, bytes)
  return bytes.byteLength
}

function parseResolveResponse (body: Buffer): ResolveArtifactsResponse {
  const parsed = JSON.parse(body.toString('utf8')) as Partial<ResolveArtifactsResponse>
  if (parsed == null || typeof parsed !== 'object' || !Array.isArray(parsed.artifacts)) {
    throw new Error('Shared artifact response has no artifacts array')
  }
  for (const artifact of parsed.artifacts) {
    validateResolvedEntry(artifact)
  }
  return parsed as ResolveArtifactsResponse
}

function validateResolvedEntry (artifact: ResolveArtifactsResponse['artifacts'][number]): void {
  if (artifact == null || typeof artifact.key !== 'string' || !Array.isArray(artifact.variants)) {
    throw new Error('Shared artifact response contains a malformed entry')
  }
  for (const variant of artifact.variants) {
    if (variant == null || variant.envelope == null || typeof variant.envelope !== 'object') {
      throw new Error('Shared artifact response contains a malformed variant')
    }
  }
}
