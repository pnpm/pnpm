import { createHash, createPrivateKey, createPublicKey, KeyObject, sign as cryptoSign, verify as cryptoVerify } from 'node:crypto'
import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'
import util, { TextDecoder } from 'node:util'

export const DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND = 'dependency-side-effects:v1'
export const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX = 'dependency-side-effects:v1:'
export const WORKSPACE_TASK_ARTIFACT_KIND = 'workspace-task:v1'
export const WORKSPACE_TASK_INPUT_KEY_PREFIX = 'workspace-task:v1:'
export const ARTIFACT_KIND = DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND
export const INPUT_KEY_PREFIX = DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX
export const COMPATIBILITY_TAG_SCHEMA = 'pnpm:v1'
export const SIGNATURE_ALGORITHM = 'ecdsa-p256-sha256'
const MAX_CANDIDATES = 2_048
const MAX_VARIANTS_PER_CANDIDATE = 8
const MAX_MANIFEST_FILES = 10_000
const MAX_FILE_SIZE = 64 * 1024 * 1024
const MAX_ARTIFACT_SIZE = 64 * 1024 * 1024
const MAX_SIGNED_PAYLOAD_SIZE = 2 * 1024 * 1024
const MAX_LOOKUP_RESPONSE_SIZE = 16 * 1024 * 1024
const MAX_PUBLISH_REQUEST_SIZE = 100 * 1024 * 1024
const MAX_BASE64_BLOB_LENGTH = Math.ceil(MAX_FILE_SIZE / 3) * 4
const MAX_BASE64_SIGNED_PAYLOAD_LENGTH = Math.ceil(MAX_SIGNED_PAYLOAD_SIZE / 3) * 4
/**
 * A canonical DER-encoded P-256 signature is a SEQUENCE of two INTEGERs of at
 * most 33 content bytes each, so it never exceeds 72 bytes.
 */
const MAX_BASE64_SIGNATURE_LENGTH = Math.ceil(72 / 3) * 4
const REQUEST_TIMEOUT = 600_000

export type OwnerScope =
  | { type: 'organization', name: string }
  | { type: 'publisher', package: string }

export type CompatibilityConstraints =
  | { kind: 'universal' }
  | { kind: 'tagged', tags: string[] }

export interface PackageIdentity {
  name: string
  version: string
}

export interface DependencySideEffectsSubject {
  kind: 'dependency-side-effects'
  package: PackageIdentity
  sourceIntegrity: string
}

export interface WorkspaceTaskSubject {
  kind: 'workspace-task'
  project: string
  task: string
}

export type ArtifactSubject = DependencySideEffectsSubject | WorkspaceTaskSubject

export interface LinuxGlibcPlatform {
  architecture: string
  nodeMajor: number
  glibcMajor: number
  glibcMinor: number
}

export interface MacOSPlatform {
  architecture: string
  nodeMajor: number
  macOSMajor: number
  macOSMinor: number
}

export interface WindowsPlatform {
  architecture: string
  nodeMajor: number
  windowsMajor: number
  windowsMinor: number
  windowsBuild: number
}

export interface BuilderProfile {
  imageDigest?: string
  architectureBaseline: string
  environment: Record<string, string>
}

export interface ArtifactFile {
  path: string
  integrity: string
  mode: number
  size: number
}

export interface ArtifactManifest {
  added: ArtifactFile[]
  deleted: string[]
}

interface ArtifactPayloadFields {
  inputKey: string
  owner: OwnerScope
  builderId: string
  builderProfile: BuilderProfile
  compatibility: CompatibilityConstraints
  manifest: ArtifactManifest
}

export type DependencySideEffectsPayload = ArtifactPayloadFields & {
  kind: typeof DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND
  subject: DependencySideEffectsSubject
}

export type WorkspaceTaskPayload = ArtifactPayloadFields & {
  kind: typeof WORKSPACE_TASK_ARTIFACT_KIND
  subject: WorkspaceTaskSubject
}

export type ArtifactPayload = DependencySideEffectsPayload | WorkspaceTaskPayload

export interface SignedArtifactEnvelope {
  algorithm: typeof SIGNATURE_ALGORITHM
  keyId: string
  payload: string
  signature: string
}

export interface ArtifactCandidate<S extends ArtifactSubject = ArtifactSubject> {
  key: string
  subject: S
  owner: OwnerScope
}

export type DependencySideEffectsCandidate = ArtifactCandidate<DependencySideEffectsSubject>

export interface ArtifactBlobUpload {
  integrity: string
  data: string
}

export interface ArtifactBlobRequest {
  owner: OwnerScope
  integrity: string
}

export interface VerifiedArtifact {
  payload: ArtifactPayload
  envelope: SignedArtifactEnvelope
  envelopeDigest: string
}

export interface VerifyStoredSharedSideEffectsOptions {
  candidate: DependencySideEffectsCandidate
  envelope: SignedArtifactEnvelope
  publicKey: string
  supportedTags: string[]
}

export interface CreateSignedArtifactEnvelopeOptions {
  keyId: string
  privateKey: string | Buffer | KeyObject
}

export interface PublishSharedSideEffectsOptions {
  registryUrl: string
  authorization?: string
  key: string
  envelope: SignedArtifactEnvelope
  blobs: ArtifactBlobUpload[]
}

export interface ResolveSharedSideEffectsOptions {
  registryUrl: string
  authorization?: string
  candidates: DependencySideEffectsCandidate[]
  supportedTags: string[]
  policy: {
    ignoreScripts: boolean
    eligiblePackages: ReadonlySet<string>
    allowedBuilds: ReadonlySet<string>
  }
  /** Base64-encoded P-256 SubjectPublicKeyInfo DER, keyed by key id. */
  trustedKeys: Record<string, string>
  quarantinedEnvelopeDigests?: ReadonlyMap<string, ReadonlySet<string>>
  onRejectedArtifact?: (rejection: {
    inputKey: string
    envelopeDigest: string
    reason: string
  }) => void
}

interface ArtifactVariant {
  envelope: SignedArtifactEnvelope
}

interface ResolveArtifactsResponse {
  artifacts: Array<{ key: string, variants: ArtifactVariant[] }>
}

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
  const candidates = new Map<string, DependencySideEffectsCandidate>()
  for (const candidate of permittedCandidates) {
    validateCandidate(candidate)
    if (candidates.has(candidate.key)) {
      throw new Error(`Duplicate shared artifact candidate ${JSON.stringify(candidate.key)}`)
    }
    candidates.set(candidate.key, candidate)
  }
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
  const selected = new Map<string, VerifiedArtifact>()
  const responseKeys = new Set<string>()
  for (const artifact of parsed.artifacts) {
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
    let best: { rank: number, artifact: VerifiedArtifact } | undefined
    for (const variant of artifact.variants) {
      const publicKey = opts.trustedKeys[variant.envelope.keyId]
      if (publicKey == null) continue
      let decoded: DecodedEnvelopeFields
      try {
        decoded = decodeEnvelopeFields(variant.envelope)
        verifyEnvelopeSignature(decoded, publicKey)
      } catch {
        continue
      }
      const digest = digestDecodedEnvelope(variant.envelope, decoded)
      if (opts.quarantinedEnvelopeDigests?.get(candidate.key)?.has(digest) === true) continue
      let payload: ArtifactPayload
      try {
        payload = decodeArtifactPayload(decoded.payloadBytes)
        validatePayload(payload)
      } catch (err: unknown) {
        opts.onRejectedArtifact?.({
          inputKey: candidate.key,
          envelopeDigest: digest,
          reason: errorMessage(err),
        })
        continue
      }
      if (
        payload.inputKey !== candidate.key ||
        !subjectsEqual(payload.subject, candidate.subject) ||
        !ownersEqual(payload.owner, candidate.owner)
      ) continue
      const rank = rankCompatibility(payload.compatibility, opts.supportedTags)
      if (rank == null) continue
      if (
        best == null ||
        rank < best.rank ||
        (rank === best.rank && digest < best.artifact.envelopeDigest)
      ) {
        best = {
          rank,
          artifact: {
            payload,
            envelope: variant.envelope,
            envelopeDigest: digest,
          },
        }
      }
    }
    if (best != null) selected.set(candidate.key, best.artifact)
  }
  return selected
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

function verifyEnvelopeSignature (
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
  const { payload } = decodeEnvelope(opts.envelope)
  const { inputKeyPrefix } = subjectArtifactIdentity(payload.subject)
  if (typeof opts.key !== 'string' || !opts.key.startsWith(inputKeyPrefix)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(inputKeyPrefix)}`)
  }
  validateScalar('input key', opts.key, 4_096)
  if (payload.inputKey !== opts.key) {
    throw new Error('Signed shared artifact input key does not match the publication key')
  }
  if (!Array.isArray(opts.blobs)) throw new Error('Shared artifact blob uploads are malformed')

  const required = new Map(payload.manifest.added.map(file => [file.integrity, file.size]))
  const uploads = new Set<string>()
  let uploadedSize = 0
  let encodedSize = Buffer.byteLength(opts.key) +
    Buffer.byteLength(opts.envelope.keyId) +
    Buffer.byteLength(opts.envelope.payload) +
    Buffer.byteLength(opts.envelope.signature)
  for (const blob of opts.blobs) {
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
    uploadedSize += bytes.byteLength
    encodedSize += Buffer.byteLength(blob.integrity) + Buffer.byteLength(blob.data)
  }
  if (uploadedSize > MAX_ARTIFACT_SIZE || encodedSize > MAX_PUBLISH_REQUEST_SIZE) {
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

interface DecodedEnvelopeFields {
  payloadBytes: Buffer
  signatureBytes: Buffer
}

function decodeEnvelopeFields (envelope: SignedArtifactEnvelope): DecodedEnvelopeFields {
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

function decodeEnvelope (envelope: SignedArtifactEnvelope): DecodedEnvelopeFields & { payload: ArtifactPayload } {
  const decoded = decodeEnvelopeFields(envelope)
  const payload = decodeArtifactPayload(decoded.payloadBytes)
  validatePayload(payload)
  return { ...decoded, payload }
}

function decodeArtifactPayload (payloadBytes: Buffer): ArtifactPayload {
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(payloadBytes)) as ArtifactPayload
}

export function signedArtifactEnvelopeDigest (envelope: SignedArtifactEnvelope): string {
  return digestDecodedEnvelope(envelope, decodeEnvelopeFields(envelope))
}

function digestDecodedEnvelope (
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

function errorMessage (err: unknown): string {
  return util.types.isNativeError(err) ? err.message : String(err)
}

function validatePayload (payload: ArtifactPayload): void {
  if (payload == null || typeof payload !== 'object') throw new Error('Shared artifact payload is not an object')
  const { artifactKind, inputKeyPrefix } = subjectArtifactIdentity(payload.subject)
  if (payload.kind !== artifactKind) throw new Error(`Unsupported shared artifact kind ${JSON.stringify(payload.kind)}`)
  if (typeof payload.inputKey !== 'string' || !payload.inputKey.startsWith(inputKeyPrefix)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(inputKeyPrefix)}`)
  }
  validateScalar('input key', payload.inputKey, 4_096)
  validateScalar('builder id', payload.builderId, 256)
  validateOwner(payload.owner)
  validateSubject(payload.subject, payload.owner)
  validateBuilderProfile(payload.builderProfile)
  validateCompatibility(payload.compatibility)
  validateManifest(payload.manifest)
}

function validateCandidate (candidate: ArtifactCandidate): void {
  if (candidate == null || typeof candidate !== 'object') throw new Error('Shared artifact candidate is malformed')
  const { inputKeyPrefix } = subjectArtifactIdentity(candidate.subject)
  if (typeof candidate.key !== 'string' || !candidate.key.startsWith(inputKeyPrefix)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(inputKeyPrefix)}`)
  }
  validateScalar('input key', candidate.key, 4_096)
  validateOwner(candidate.owner)
  validateSubject(candidate.subject, candidate.owner)
}

function subjectArtifactIdentity (subject: ArtifactSubject): {
  artifactKind: typeof DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND | typeof WORKSPACE_TASK_ARTIFACT_KIND
  inputKeyPrefix: typeof DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX | typeof WORKSPACE_TASK_INPUT_KEY_PREFIX
} {
  if (subject == null || typeof subject !== 'object') throw new Error('Shared artifact subject is malformed')
  if (subject.kind === 'dependency-side-effects') {
    return {
      artifactKind: DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND,
      inputKeyPrefix: DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX,
    }
  }
  if (subject.kind === 'workspace-task') {
    return {
      artifactKind: WORKSPACE_TASK_ARTIFACT_KIND,
      inputKeyPrefix: WORKSPACE_TASK_INPUT_KEY_PREFIX,
    }
  }
  throw new Error(`Unsupported shared artifact subject ${JSON.stringify((subject as { kind?: unknown }).kind)}`)
}

function validateSubject (subject: ArtifactSubject, owner: OwnerScope): void {
  subjectArtifactIdentity(subject)
  if (subject.kind === 'dependency-side-effects') {
    validatePackageIdentity(subject.package)
    validateScalar('source integrity', subject.sourceIntegrity, 1_024)
    validatePublisherPackage(owner, subject.package)
    return
  }
  validateScalar('workspace project', subject.project, 4_096)
  validateScalar('workspace task', subject.task, 256)
  if (owner.type === 'publisher') {
    throw new Error('Workspace task artifacts require an organization owner')
  }
}

function subjectsEqual (left: ArtifactSubject, right: ArtifactSubject): boolean {
  if (left.kind !== right.kind) return false
  if (left.kind === 'dependency-side-effects' && right.kind === 'dependency-side-effects') {
    return left.package.name === right.package.name &&
      left.package.version === right.package.version &&
      left.sourceIntegrity === right.sourceIntegrity
  }
  return left.kind === 'workspace-task' && right.kind === 'workspace-task' &&
    left.project === right.project && left.task === right.task
}

function validatePackageIdentity (packageIdentity: PackageIdentity): void {
  if (packageIdentity == null || typeof packageIdentity !== 'object') {
    throw new Error('Shared artifact package identity is malformed')
  }
  validateScalar('package name', packageIdentity.name, 256)
  validateScalar('package version', packageIdentity.version, 256)
}

function validatePublisherPackage (owner: OwnerScope, packageIdentity: PackageIdentity): void {
  if (owner.type === 'publisher' && owner.package !== packageIdentity.name) {
    throw new Error('Shared artifact publisher owner does not match the signed package name')
  }
}

function validateOwner (owner: OwnerScope): void {
  if (owner?.type === 'organization') validateScalar('organization owner', owner.name, 256)
  else if (owner?.type === 'publisher') validateScalar('publisher owner', owner.package, 256)
  else throw new Error('Shared artifact owner has an unknown type')
}

function validateBuilderProfile (profile: BuilderProfile): void {
  if (profile == null || typeof profile !== 'object') throw new Error('Shared artifact builder profile is not an object')
  if (profile.imageDigest != null) validateScalar('builder image digest', profile.imageDigest, 1_024)
  validateScalar('architecture baseline', profile.architectureBaseline, 256)
  if (profile.environment == null || typeof profile.environment !== 'object' || Array.isArray(profile.environment)) {
    throw new Error('Shared artifact builder environment is not an object')
  }
  const entries = Object.entries(profile.environment)
  if (entries.length > 128) throw new Error('Shared artifact builder environment contains more than 128 variables')
  for (const [name, value] of entries) {
    validateScalar('builder environment name', name, 256)
    validateScalar('builder environment value', value, 4_096)
  }
}

function validateCompatibility (compatibility: CompatibilityConstraints): void {
  if (compatibility?.kind === 'universal') return
  if (compatibility?.kind !== 'tagged' || !Array.isArray(compatibility.tags)) {
    throw new Error('Shared artifact compatibility has an unknown kind')
  }
  if (compatibility.tags.length === 0 || compatibility.tags.length > 64) {
    throw new Error('Tagged compatibility must contain between 1 and 64 tags')
  }
  const unique = new Set<string>()
  for (const tag of compatibility.tags) {
    validateCompatibilityTag(tag)
    if (unique.has(tag)) throw new Error(`Duplicate compatibility tag ${JSON.stringify(tag)}`)
    unique.add(tag)
  }
}

function validateManifest (manifest: ArtifactManifest): void {
  if (manifest == null || !Array.isArray(manifest.added) || !Array.isArray(manifest.deleted)) {
    throw new Error('Shared artifact manifest is malformed')
  }
  const fileCount = manifest.added.length + manifest.deleted.length
  if (fileCount > MAX_MANIFEST_FILES) {
    throw new Error(`Shared artifact manifest contains ${fileCount} paths; limit is ${MAX_MANIFEST_FILES}`)
  }
  const exactPaths = new Set<string>()
  const foldedPaths = new Set<string>()
  const integritySizes = new Map<string, number>()
  let totalSize = 0
  for (const file of manifest.added) {
    if (file == null || typeof file !== 'object') throw new Error('Shared artifact file entry is malformed')
    validateManifestPath(file.path)
    insertUniquePath(file.path, exactPaths, foldedPaths)
    if (file.mode !== 0o644 && file.mode !== 0o755) {
      throw new Error(`Shared artifact path ${JSON.stringify(file.path)} has unsupported mode ${String(file.mode)}`)
    }
    if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_FILE_SIZE) {
      throw new Error(`Shared artifact path ${JSON.stringify(file.path)} has an invalid size`)
    }
    totalSize += file.size
    if (totalSize > MAX_ARTIFACT_SIZE) throw new Error(`Shared artifact exceeds ${MAX_ARTIFACT_SIZE} bytes`)
    artifactBlobDigest(file.integrity)
    const previousSize = integritySizes.get(file.integrity)
    if (previousSize != null && previousSize !== file.size) {
      throw new Error(`Shared artifact blob integrity ${JSON.stringify(file.integrity)} is declared with inconsistent sizes`)
    }
    integritySizes.set(file.integrity, file.size)
  }
  for (const path of manifest.deleted) {
    validateManifestPath(path)
    insertUniquePath(path, exactPaths, foldedPaths)
  }
}

function validateManifestPath (path: string): void {
  if (typeof path !== 'string' || path.length === 0 || Buffer.byteLength(path) > 4_096) {
    throw new Error('Shared artifact path length is outside the allowed range')
  }
  if (path.startsWith('/') || path.startsWith('\\') || path[1] === ':') {
    throw new Error(`Shared artifact path ${JSON.stringify(path)} is absolute`)
  }
  if (path.includes('\\')) throw new Error(`Shared artifact path ${JSON.stringify(path)} uses a backslash separator`)
  if (Array.from(path).some(character => isControl(character))) {
    throw new Error(`Shared artifact path ${JSON.stringify(path)} contains a control character`)
  }
  if (path.split('/').some(segment =>
    segment === '' ||
    segment === '.' ||
    segment === '..' ||
    segment.includes(':') ||
    isWindowsReservedName(segment) ||
    segment.endsWith('.') ||
    segment.endsWith(' ')
  )) {
    throw new Error(`Shared artifact path ${JSON.stringify(path)} has an empty, dot, parent, or Windows-normalized segment`)
  }
}

function isWindowsReservedName (segment: string): boolean {
  const basename = segment.split('.')[0].toLowerCase()
  if (['con', 'prn', 'aux', 'nul'].includes(basename)) return true
  return ['com', 'lpt'].some(prefix => {
    const suffix = basename.startsWith(prefix) ? basename.slice(prefix.length) : ''
    return (suffix.length === 1 && suffix >= '1' && suffix <= '9') || ['¹', '²', '³'].includes(suffix)
  })
}

function insertUniquePath (path: string, exact: Set<string>, folded: Set<string>): void {
  if (exact.has(path)) throw new Error(`Duplicate shared artifact path ${JSON.stringify(path)}`)
  const caseFolded = path.toLowerCase()
  if (folded.has(caseFolded)) {
    throw new Error(`Shared artifact path ${JSON.stringify(path)} collides on a case-insensitive filesystem`)
  }
  exact.add(path)
  folded.add(caseFolded)
}

export function compatibilityRank (constraints: CompatibilityConstraints, supportedTags: string[]): number | undefined {
  try {
    validateSupportedTags(supportedTags)
  } catch {
    return undefined
  }
  return rankCompatibility(constraints, supportedTags)
}

function rankCompatibility (constraints: CompatibilityConstraints, supportedTags: string[]): number | undefined {
  if (constraints.kind === 'universal') return Number.MAX_SAFE_INTEGER
  let bestRank: number | undefined
  for (let index = 0; index < supportedTags.length; index++) {
    const supportedTag = supportedTags[index]
    for (const artifactTag of constraints.tags) {
      if (artifactTag === supportedTag) {
        bestRank = Math.min(bestRank ?? index, index)
        continue
      }
      let consumer: VersionedPlatform | undefined
      let artifact: VersionedPlatform | undefined
      try {
        consumer = parseVersionedCompatibilityTag(supportedTag)
        artifact = parseVersionedCompatibilityTag(artifactTag)
      } catch {
        return undefined
      }
      if (
        consumer == null ||
        artifact == null ||
        consumer.kind !== artifact.kind ||
        consumer.platform.architecture !== artifact.platform.architecture ||
        consumer.platform.nodeMajor !== artifact.platform.nodeMajor
      ) continue
      const consumerVersion = versionedPlatformRank(consumer)
      const artifactVersion = versionedPlatformRank(artifact)
      if (consumerVersion < artifactVersion) continue
      const rank = 64 + index * 1_000_000_000_000 + consumerVersion - artifactVersion
      bestRank = Math.min(bestRank ?? rank, rank)
    }
  }
  return bestRank
}

export function linuxGlibcCompatibilityTag (
  platform: LinuxGlibcPlatform
): string {
  const { architecture, nodeMajor, glibcMajor, glibcMinor } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:linux-${architecture}-node${nodeMajor}-glibc${glibcMajor}.${glibcMinor}`
  validateCompatibilityTag(tag)
  return tag
}

export function linuxGlibcSupportedTags (
  platform: LinuxGlibcPlatform
): string[] {
  const { architecture, nodeMajor, glibcMajor, glibcMinor } = platform
  if (!Number.isSafeInteger(glibcMinor) || glibcMinor < 0 || glibcMinor >= 64) {
    throw new Error('Shared artifact glibc floor expansion exceeds 64 tags')
  }
  return Array.from(
    { length: glibcMinor + 1 },
    (_, index) => linuxGlibcCompatibilityTag({
      architecture,
      nodeMajor,
      glibcMajor,
      glibcMinor: glibcMinor - index,
    })
  )
}

export function macOSSupportedTags (platform: MacOSPlatform): string[] {
  return [macOSCompatibilityTag(platform)]
}

export function macOSCompatibilityTag (
  platform: MacOSPlatform
): string {
  const { architecture, nodeMajor, macOSMajor, macOSMinor } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:darwin-${architecture}-node${nodeMajor}-macos${macOSMajor}.${macOSMinor}`
  validateCompatibilityTag(tag)
  return tag
}

export function windowsSupportedTags (platform: WindowsPlatform): string[] {
  return [windowsCompatibilityTag(platform)]
}

export function windowsCompatibilityTag (
  platform: WindowsPlatform
): string {
  const { architecture, nodeMajor, windowsMajor, windowsMinor, windowsBuild } = platform
  const tag = `${COMPATIBILITY_TAG_SCHEMA}:win32-${architecture}-node${nodeMajor}-windows${windowsMajor}.${windowsMinor}.${windowsBuild}`
  validateCompatibilityTag(tag)
  return tag
}

export function platformFingerprint (supportedTags: string[]): string {
  validateSupportedTags(supportedTags)
  const hash = createHash('sha256').update('pnpm-platform-fingerprint-v1\0')
  for (const tag of supportedTags) hash.update(tag).update('\0')
  return hash.digest('hex')
}

function validateSupportedTags (tags: string[]): void {
  if (!Array.isArray(tags) || tags.length > 64) {
    throw new Error('Shared artifact consumer advertises more than 64 supported tags')
  }
  const unique = new Set<string>()
  for (const tag of tags) {
    validateCompatibilityTag(tag)
    if (unique.has(tag)) throw new Error(`Duplicate consumer compatibility tag ${JSON.stringify(tag)}`)
    unique.add(tag)
  }
}

function validateCompatibilityTag (tag: string): void {
  validateScalar('compatibility tag', tag, 512)
  if (!tag.startsWith(`${COMPATIBILITY_TAG_SCHEMA}:`)) {
    throw new Error('Shared artifact compatibility tag uses an unknown schema')
  }
  const parts = tag.slice(COMPATIBILITY_TAG_SCHEMA.length + 1).split('-')
  if (parts.length !== 4) throw new Error('Shared artifact compatibility tag has the wrong number of dimensions')
  const [os, architecture, node, runtime] = parts
  if (!['x64', 'arm64'].includes(architecture)) {
    throw new Error('Shared artifact compatibility tag only supports x64 and arm64 in v1')
  }
  parseCanonicalNumber(node.startsWith('node') ? node.slice(4) : '', 'Node major version', false)
  if (os === 'linux') {
    const glibc = runtime.startsWith('glibc') ? runtime.slice(5) : ''
    const version = glibc.split('.')
    if (version.length !== 2) throw new Error('Shared artifact glibc floor must be major.minor')
    parseCanonicalNumber(version[0], 'glibc major version', false)
    parseCanonicalNumber(version[1], 'glibc minor version', true)
  } else if (os === 'darwin') {
    const macOS = runtime.startsWith('macos') ? runtime.slice(5) : ''
    const version = macOS.split('.')
    if (version.length !== 2) throw new Error('Shared artifact macOS floor must be major.minor')
    const major = parseCanonicalNumber(version[0], 'macOS major version', false)
    const minor = parseCanonicalNumber(version[1], 'macOS minor version', true)
    if (major >= 1_000_000 || minor >= 1_000_000) {
      throw new Error('Shared artifact macOS version component is too large')
    }
  } else if (os === 'win32') {
    const windows = runtime.startsWith('windows') ? runtime.slice(7) : ''
    const version = windows.split('.')
    if (version.length !== 3) throw new Error('Shared artifact Windows floor must be major.minor.build')
    const major = parseCanonicalNumber(version[0], 'Windows major version', false)
    const minor = parseCanonicalNumber(version[1], 'Windows minor version', true)
    const build = parseCanonicalNumber(version[2], 'Windows build number', false)
    if (major >= 1_000 || minor >= 1_000 || build >= 1_000_000) {
      throw new Error('Shared artifact Windows version component is too large')
    }
  } else {
    throw new Error('Shared artifact compatibility tag only supports Linux, macOS, and Windows in v1')
  }
}

type VersionedPlatform =
  | { kind: 'macOS', platform: MacOSPlatform }
  | { kind: 'windows', platform: WindowsPlatform }

function parseVersionedCompatibilityTag (tag: string): VersionedPlatform | undefined {
  validateCompatibilityTag(tag)
  const [os, architecture, node, runtime] = tag.slice(COMPATIBILITY_TAG_SCHEMA.length + 1).split('-')
  const nodeMajor = Number(node.slice(4))
  if (os === 'darwin') {
    const [macOSMajor, macOSMinor] = runtime.slice(5).split('.').map(Number)
    return {
      kind: 'macOS',
      platform: { architecture, nodeMajor, macOSMajor, macOSMinor },
    }
  }
  if (os === 'win32') {
    const [windowsMajor, windowsMinor, windowsBuild] = runtime.slice(7).split('.').map(Number)
    return {
      kind: 'windows',
      platform: { architecture, nodeMajor, windowsMajor, windowsMinor, windowsBuild },
    }
  }
  return undefined
}

function versionedPlatformRank (versionedPlatform: VersionedPlatform): number {
  if (versionedPlatform.kind === 'macOS') {
    return versionedPlatform.platform.macOSMajor * 1_000_000 + versionedPlatform.platform.macOSMinor
  }
  return (
    versionedPlatform.platform.windowsMajor * 1_000_000_000 +
    versionedPlatform.platform.windowsMinor * 1_000_000 +
    versionedPlatform.platform.windowsBuild
  )
}

function parseCanonicalNumber (value: string, label: string, allowZero: boolean): number {
  if (value.length === 0 || Array.from(value).some(character => character < '0' || character > '9')) {
    throw new Error(`Shared artifact compatibility tag has an invalid ${label}`)
  }
  const number = Number(value)
  if (!Number.isSafeInteger(number) || String(number) !== value || (!allowZero && number === 0)) {
    throw new Error(`Shared artifact compatibility tag has a non-canonical ${label}`)
  }
  return number
}

function ownersEqual (left: OwnerScope, right: OwnerScope): boolean {
  if (left.type !== right.type) return false
  return left.type === 'organization'
    ? left.name === (right as { type: 'organization', name: string }).name
    : left.package === (right as { type: 'publisher', package: string }).package
}

/**
 * Hex digest identifying an artifact blob's content, from its `sha512-` value.
 *
 * The same identity the store addresses its content by, so a caller holding a
 * manifest entry can ask the store whether it already has the bytes. Callers
 * that only need the integrity checked can discard the result.
 *
 * @throws if `integrity` is not a `sha512-` value carrying a 64-byte digest.
 */
export function artifactBlobDigest (integrity: string): string {
  if (typeof integrity !== 'string' || !integrity.startsWith('sha512-')) {
    throw new Error('Shared artifact blobs require sha512 integrity')
  }
  const digest = decodeBase64('sha512 digest', integrity.slice('sha512-'.length))
  if (digest.byteLength !== 64) throw new Error(`SHA-512 digest is ${digest.byteLength} bytes instead of 64`)
  return digest.toString('hex')
}

function verifyBlob (integrity: string, bytes: Buffer): void {
  const expected = artifactBlobDigest(integrity)
  const actual = createHash('sha512').update(bytes).digest('hex')
  if (expected !== actual) throw new Error('Downloaded shared artifact blob does not match its declared digest')
}

function validateScalar (label: string, value: unknown, maxLength: number): asserts value is string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    Buffer.byteLength(value) > maxLength ||
    Array.from(value).some(character => isControl(character))
  ) {
    throw new Error(`Shared artifact ${label} is empty, too long, or contains a control character`)
  }
}

function isControl (character: string): boolean {
  const codePoint = character.codePointAt(0)!
  return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f)
}

function decodeBase64 (label: string, encoded: string, allowEmpty = false): Buffer {
  if (typeof encoded !== 'string' || (!allowEmpty && encoded.length === 0) || /\s/.test(encoded)) {
    throw new Error(`Shared artifact ${label} is not valid base64`)
  }
  const decoded = Buffer.from(encoded, 'base64')
  if (decoded.toString('base64') !== encoded) {
    throw new Error(`Shared artifact ${label} is not valid base64`)
  }
  return decoded
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
    if (
      length === 0 ||
      length > 33 ||
      end > signature.length ||
      (signature[start] & 0x80) !== 0 ||
      (length > 1 && signature[start] === 0 && (signature[start + 1] & 0x80) === 0)
    ) {
      throw new Error('Shared artifact signature is not canonical P-256 DER')
    }
    offset = end
  }
  if (offset !== signature.length) throw new Error('Shared artifact signature is not canonical P-256 DER')
}

function parseResolveResponse (body: Buffer): ResolveArtifactsResponse {
  const parsed = JSON.parse(body.toString('utf8')) as Partial<ResolveArtifactsResponse>
  if (parsed == null || typeof parsed !== 'object' || !Array.isArray(parsed.artifacts)) {
    throw new Error('Shared artifact response has no artifacts array')
  }
  for (const artifact of parsed.artifacts) {
    if (artifact == null || typeof artifact.key !== 'string' || !Array.isArray(artifact.variants)) {
      throw new Error('Shared artifact response contains a malformed entry')
    }
    for (const variant of artifact.variants) {
      if (variant == null || variant.envelope == null || typeof variant.envelope !== 'object') {
        throw new Error('Shared artifact response contains a malformed variant')
      }
    }
  }
  return parsed as ResolveArtifactsResponse
}

interface RequestOptions {
  registryUrl: string
  path: string
  method: 'GET' | 'POST' | 'PUT'
  authorization?: string
  body?: Buffer
  maxResponseSize: number
}

interface BufferedResponse {
  statusCode: number
  body: Buffer
}

async function request (opts: RequestOptions): Promise<BufferedResponse> {
  const base = opts.registryUrl.endsWith('/') ? opts.registryUrl : `${opts.registryUrl}/`
  const url = new URL(opts.path, base)
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error(`Unsupported pnpr registry protocol ${JSON.stringify(url.protocol)}`)
  }
  const requestFn = url.protocol === 'https:' ? https.request : http.request
  const headers: http.OutgoingHttpHeaders = {}
  if (opts.body != null) {
    headers['Content-Type'] = 'application/json'
    headers['Content-Length'] = opts.body.byteLength
  }
  if (opts.authorization != null) headers.Authorization = opts.authorization

  return new Promise((resolve, reject) => {
    const req = requestFn(url, {
      method: opts.method,
      timeout: REQUEST_TIMEOUT,
      headers,
    }, (res) => {
      const declaredLength = Number(res.headers['content-length'])
      if (Number.isFinite(declaredLength) && declaredLength > opts.maxResponseSize) {
        res.destroy(new Error(`pnpr response exceeds ${opts.maxResponseSize} bytes`))
        return
      }
      const chunks: Buffer[] = []
      let received = 0
      res.on('data', (chunk: Buffer) => {
        received += chunk.byteLength
        if (received > opts.maxResponseSize) {
          res.destroy(new Error(`pnpr response exceeds ${opts.maxResponseSize} bytes`))
          return
        }
        chunks.push(chunk)
      })
      res.on('end', () => resolve({ statusCode: res.statusCode ?? 0, body: Buffer.concat(chunks) }))
      res.on('error', reject)
    })
    req.on('timeout', () => req.destroy(new Error(`pnpr server request timed out after ${REQUEST_TIMEOUT / 1000}s`)))
    req.on('error', reject)
    req.end(opts.body)
  })
}

function assertSuccess (response: BufferedResponse, endpoint: string): void {
  if (response.statusCode < 200 || response.statusCode >= 300) {
    const body = response.body.toString('utf8').slice(0, 1_024)
    throw new Error(`pnpr server ${endpoint} responded with ${response.statusCode}: ${body}`)
  }
}
