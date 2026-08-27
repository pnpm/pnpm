import { createHash, createPrivateKey, createPublicKey, KeyObject, sign as cryptoSign, verify as cryptoVerify } from 'node:crypto'
import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'
import { TextDecoder } from 'node:util'

export const ARTIFACT_KIND = 'dependency-side-effects:v1'
export const INPUT_KEY_PREFIX = 'dependency-side-effects:v1:'
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

export interface LinuxGlibcPlatform {
  architecture: string
  nodeMajor: number
  glibcMajor: number
  glibcMinor: number
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

export interface ArtifactPayload {
  kind: typeof ARTIFACT_KIND
  package: PackageIdentity
  sourceIntegrity: string
  inputKey: string
  owner: OwnerScope
  builderId: string
  builderProfile: BuilderProfile
  compatibility: CompatibilityConstraints
  manifest: ArtifactManifest
}

export interface SignedArtifactEnvelope {
  algorithm: typeof SIGNATURE_ALGORITHM
  keyId: string
  payload: string
  signature: string
}

export interface ArtifactCandidate {
  key: string
  package: PackageIdentity
  sourceIntegrity: string
  owner: OwnerScope
}

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
  candidates: ArtifactCandidate[]
  supportedTags: string[]
  policy: {
    ignoreScripts: boolean
    eligiblePackages: ReadonlySet<string>
    allowedBuilds: ReadonlySet<string>
  }
  /** Base64-encoded P-256 SubjectPublicKeyInfo DER, keyed by key id. */
  trustedKeys: Record<string, string>
  pinnedEnvelopeDigests?: ReadonlyMap<string, string>
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
    opts.policy.eligiblePackages.has(candidate.package.name) && opts.policy.allowedBuilds.has(candidate.package.name)
  )
  if (permittedCandidates.length === 0) return new Map()
  if (permittedCandidates.length > MAX_CANDIDATES) {
    throw new Error(`Shared artifact lookup exceeds the ${MAX_CANDIDATES}-candidate limit`)
  }
  const candidates = new Map<string, ArtifactCandidate>()
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
      let payload: ArtifactPayload
      try {
        payload = verifySignedArtifactEnvelope(variant.envelope, publicKey)
      } catch {
        continue
      }
      if (
        payload.inputKey !== candidate.key ||
        payload.package.name !== candidate.package.name ||
        payload.package.version !== candidate.package.version ||
        payload.sourceIntegrity !== candidate.sourceIntegrity ||
        !ownersEqual(payload.owner, candidate.owner)
      ) continue
      const rank = compatibilityRank(payload.compatibility, opts.supportedTags)
      if (rank == null) continue
      const digest = signedArtifactEnvelopeDigest(variant.envelope)
      const pinnedDigest = opts.pinnedEnvelopeDigests?.get(candidate.key)
      if (pinnedDigest != null && digest !== pinnedDigest) continue
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

export function verifySignedArtifactEnvelope (
  envelope: SignedArtifactEnvelope,
  publicKeySpki: string
): ArtifactPayload {
  const { payload, payloadBytes, signatureBytes } = decodeEnvelope(envelope)
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
  return payload
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
  verifyBlob(opts.request.integrity, response.body)
  return response.body
}

function serializePublishRequest (opts: PublishSharedSideEffectsOptions): Buffer {
  if (typeof opts.key !== 'string' || !opts.key.startsWith(INPUT_KEY_PREFIX)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(INPUT_KEY_PREFIX)}`)
  }
  validateScalar('input key', opts.key, 4_096)
  const { payload } = decodeEnvelope(opts.envelope)
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

function decodeEnvelope (envelope: SignedArtifactEnvelope): {
  payload: ArtifactPayload
  payloadBytes: Buffer
  signatureBytes: Buffer
} {
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
  const payload = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(payloadBytes)) as ArtifactPayload
  validatePayload(payload)
  return { payload, payloadBytes, signatureBytes }
}

export function signedArtifactEnvelopeDigest (envelope: SignedArtifactEnvelope): string {
  const { payloadBytes, signatureBytes } = decodeEnvelope(envelope)
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

function validatePayload (payload: ArtifactPayload): void {
  if (payload == null || typeof payload !== 'object') throw new Error('Shared artifact payload is not an object')
  if (payload.kind !== ARTIFACT_KIND) throw new Error(`Unsupported shared artifact kind ${JSON.stringify(payload.kind)}`)
  if (typeof payload.inputKey !== 'string' || !payload.inputKey.startsWith(INPUT_KEY_PREFIX)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(INPUT_KEY_PREFIX)}`)
  }
  validateScalar('input key', payload.inputKey, 4_096)
  validatePackageIdentity(payload.package)
  validateScalar('source integrity', payload.sourceIntegrity, 1_024)
  validateScalar('builder id', payload.builderId, 256)
  validateOwner(payload.owner)
  validatePublisherPackage(payload.owner, payload.package)
  validateBuilderProfile(payload.builderProfile)
  validateCompatibility(payload.compatibility)
  validateManifest(payload.manifest)
}

function validateCandidate (candidate: ArtifactCandidate): void {
  if (candidate == null || typeof candidate !== 'object') throw new Error('Shared artifact candidate is malformed')
  if (typeof candidate.key !== 'string' || !candidate.key.startsWith(INPUT_KEY_PREFIX)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(INPUT_KEY_PREFIX)}`)
  }
  validateScalar('input key', candidate.key, 4_096)
  validatePackageIdentity(candidate.package)
  validateScalar('source integrity', candidate.sourceIntegrity, 1_024)
  validateOwner(candidate.owner)
  validatePublisherPackage(candidate.owner, candidate.package)
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

function compatibilityRank (constraints: CompatibilityConstraints, supportedTags: string[]): number | undefined {
  if (constraints.kind === 'universal') return supportedTags.length
  for (let index = 0; index < supportedTags.length; index++) {
    if (constraints.tags.includes(supportedTags[index])) return index
  }
  return undefined
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
  const [os, architecture, node, libc] = parts
  if (os !== 'linux' || !['x64', 'arm64'].includes(architecture)) {
    throw new Error('Shared artifact compatibility tag only supports Linux x64 and arm64 in v1')
  }
  parseCanonicalNumber(node.startsWith('node') ? node.slice(4) : '', 'Node major version', false)
  const glibc = libc.startsWith('glibc') ? libc.slice(5) : ''
  const version = glibc.split('.')
  if (version.length !== 2) throw new Error('Shared artifact glibc floor must be major.minor')
  parseCanonicalNumber(version[0], 'glibc major version', false)
  parseCanonicalNumber(version[1], 'glibc minor version', true)
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
