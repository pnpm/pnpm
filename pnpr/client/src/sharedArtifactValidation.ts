import { SYMLINK_MODE } from '@pnpm/store.cafs'

import { artifactBlobDigest, isControl, validateScalar } from './artifactEncoding.js'
import { validateCompatibility } from './compatibilityTags.js'
import {
  type ArtifactCandidate,
  type ArtifactFile,
  type ArtifactManifest,
  type ArtifactPayload,
  type ArtifactSubject,
  type BuilderProfile,
  DEPENDENCY_SIDE_EFFECTS_ARTIFACT_KIND,
  DEPENDENCY_SIDE_EFFECTS_INPUT_KEY_PREFIX,
  MAX_ARTIFACT_SIZE,
  MAX_FILE_SIZE,
  MAX_MANIFEST_FILES,
  type OwnerScope,
  type PackageIdentity,
  WORKSPACE_TASK_ARTIFACT_KIND,
  WORKSPACE_TASK_INPUT_KEY_PREFIX,
} from './sharedArtifactTypes.js'

interface ManifestPaths {
  exact: Set<string>
  folded: Set<string>
}

export function validatePayload (payload: ArtifactPayload): void {
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

export function validateCandidate (candidate: ArtifactCandidate): void {
  if (candidate == null || typeof candidate !== 'object') throw new Error('Shared artifact candidate is malformed')
  const { inputKeyPrefix } = subjectArtifactIdentity(candidate.subject)
  if (typeof candidate.key !== 'string' || !candidate.key.startsWith(inputKeyPrefix)) {
    throw new Error(`Shared artifact input key must start with ${JSON.stringify(inputKeyPrefix)}`)
  }
  validateScalar('input key', candidate.key, 4_096)
  validateOwner(candidate.owner)
  validateSubject(candidate.subject, candidate.owner)
}

export function subjectArtifactIdentity (subject: ArtifactSubject): {
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

export function subjectsEqual (left: ArtifactSubject, right: ArtifactSubject): boolean {
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

export function validateOwner (owner: OwnerScope): void {
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

function validateManifest (manifest: ArtifactManifest): void {
  if (manifest == null || !Array.isArray(manifest.added) || !Array.isArray(manifest.deleted)) {
    throw new Error('Shared artifact manifest is malformed')
  }
  const fileCount = manifest.added.length + manifest.deleted.length
  if (fileCount > MAX_MANIFEST_FILES) {
    throw new Error(`Shared artifact manifest contains ${fileCount} paths; limit is ${MAX_MANIFEST_FILES}`)
  }
  const paths: ManifestPaths = { exact: new Set<string>(), folded: new Set<string>() }
  const integritySizes = new Map<string, number>()
  let totalSize = 0
  for (const file of manifest.added) {
    validateAddedFile(file, paths)
    totalSize += file.size
    if (totalSize > MAX_ARTIFACT_SIZE) throw new Error(`Shared artifact exceeds ${MAX_ARTIFACT_SIZE} bytes`)
    validateBlobDeclaration(file, integritySizes)
  }
  for (const path of manifest.deleted) {
    validateManifestPath(path)
    insertUniquePath(path, paths.exact, paths.folded)
  }
}

function validateAddedFile (file: ArtifactFile, paths: ManifestPaths): void {
  if (file == null || typeof file !== 'object') throw new Error('Shared artifact file entry is malformed')
  validateManifestPath(file.path)
  insertUniquePath(file.path, paths.exact, paths.folded)
  if (file.mode !== 0o644 && file.mode !== 0o755 && file.mode !== SYMLINK_MODE) {
    throw new Error(`Shared artifact path ${JSON.stringify(file.path)} has unsupported mode ${String(file.mode)}`)
  }
  if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_FILE_SIZE) {
    throw new Error(`Shared artifact path ${JSON.stringify(file.path)} has an invalid size`)
  }
}

function validateBlobDeclaration (file: ArtifactFile, integritySizes: Map<string, number>): void {
  artifactBlobDigest(file.integrity)
  const previousSize = integritySizes.get(file.integrity)
  if (previousSize != null && previousSize !== file.size) {
    throw new Error(`Shared artifact blob integrity ${JSON.stringify(file.integrity)} is declared with inconsistent sizes`)
  }
  integritySizes.set(file.integrity, file.size)
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

export function ownersEqual (left: OwnerScope, right: OwnerScope): boolean {
  if (left.type !== right.type) return false
  return left.type === 'organization'
    ? left.name === (right as { type: 'organization', name: string }).name
    : left.package === (right as { type: 'publisher', package: string }).package
}
