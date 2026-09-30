import type { KeyObject } from 'node:crypto'

export const DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND = 'dependency-side-effects:v1'
export const DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX = 'dependency-side-effects:v1:'
export const WORKSPACE_TASK_ARTIFACT_KIND = 'workspace-task:v1'
export const WORKSPACE_TASK_INPUT_KEY_PREFIX = 'workspace-task:v1:'
export const ARTIFACT_KIND = DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND
export const INPUT_KEY_PREFIX = DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX
export const COMPATIBILITY_TAG_SCHEMA = 'pnpm:v1'
export const SIGNATURE_ALGORITHM = 'ecdsa-p256-sha256'
export const MAX_CANDIDATES = 2_048
export const MAX_VARIANTS_PER_CANDIDATE = 8
export const MAX_MANIFEST_FILES = 10_000
export const MAX_FILE_SIZE = 64 * 1024 * 1024
export const MAX_ARTIFACT_SIZE = 64 * 1024 * 1024
export const MAX_SIGNED_PAYLOAD_SIZE = 2 * 1024 * 1024
export const MAX_LOOKUP_RESPONSE_SIZE = 16 * 1024 * 1024
export const MAX_PUBLISH_REQUEST_SIZE = 100 * 1024 * 1024
export const MAX_BASE64_BLOB_LENGTH = Math.ceil(MAX_FILE_SIZE / 3) * 4
export const MAX_BASE64_SIGNED_PAYLOAD_LENGTH = Math.ceil(MAX_SIGNED_PAYLOAD_SIZE / 3) * 4
/**
 * A canonical DER-encoded P-256 signature is a SEQUENCE of two INTEGERs of at
 * most 33 content bytes each, so it never exceeds 72 bytes.
 */
export const MAX_BASE64_SIGNATURE_LENGTH = Math.ceil(72 / 3) * 4
export const REQUEST_TIMEOUT = 600_000

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

export interface ArtifactCandidate<Subject extends ArtifactSubject = ArtifactSubject> {
  key: string
  subject: Subject
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

export interface ArtifactVariant {
  envelope: SignedArtifactEnvelope
}

export interface ResolveArtifactsResponse {
  artifacts: Array<{ key: string, variants: ArtifactVariant[] }>
}
