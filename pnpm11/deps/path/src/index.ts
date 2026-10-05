import { createShortHash } from '@pnpm/crypto.hash'
import type { DepPath, PkgId, PkgIdWithPatchHash, PkgResolutionId, RegistriesByScope } from '@pnpm/types'
import semver from 'semver'

export { findHoistedPackageDirs, withCollapsedVariants } from './hoistedPackageDirs.js'

export function isAbsolute (dependencyPath: string): boolean {
  return dependencyPath[0] !== '/'
}

/**
 * Version-slot prefixes with a fixed meaning in dep paths, resolution ids, or
 * dependency specifiers (`foo@file:...`, `node@runtime:...`, `npm:`, ...).
 * A named-registry alias may not shadow any of them: the registry-qualified
 * dep path form `<name>@<alias>:<version>` would otherwise be ambiguous.
 */
export const RESERVED_VERSION_PREFIXES: ReadonlySet<string> = new Set([
  'bitbucket',
  'catalog',
  'custom',
  'file',
  'git',
  'github',
  'gitlab',
  'http',
  'https',
  'jsr',
  'link',
  'npm',
  'runtime',
  'ssh',
  'workspace',
])

const REGISTRY_NAME_RE = /^[A-Z][\w.-]*$/i

/**
 * Whether `name` is syntactically usable as a named-registry alias in a
 * registry-qualified dep path. Does not check the reserved list —
 * see `RESERVED_VERSION_PREFIXES` for that.
 */
export function isWellFormedRegistryName (name: string): boolean {
  return REGISTRY_NAME_RE.test(name)
}

export interface RegistryQualifiedVersion {
  registryName: string
  version: string
}

/**
 * Parses the version slot of a registry-qualified dep path
 * (`<registryName>:<version>`, e.g. `work:1.0.0`) — the form used for
 * packages resolved from a named registry since lockfile format 12.0.
 * Returns `undefined` for every other version form (plain semver, `file:`,
 * `runtime:`, git/tarball URLs, ...).
 */
export function parseRegistryQualifiedVersion (version: string): RegistryQualifiedVersion | undefined {
  const colonIndex = version.indexOf(':')
  if (colonIndex < 1) return undefined
  const registryName = version.substring(0, colonIndex)
  if (RESERVED_VERSION_PREFIXES.has(registryName) || !REGISTRY_NAME_RE.test(registryName)) return undefined
  const qualifiedVersion = version.substring(colonIndex + 1)
  if (semver.valid(qualifiedVersion) == null) return undefined
  return { registryName, version: qualifiedVersion }
}

export function indexOfDepPathSuffix (depPath: string): { peersIndex: number, patchHashIndex: number } {
  if (!depPath.endsWith(')')) return { peersIndex: -1, patchHashIndex: -1 }
  let open = 1
  for (let charIndex = depPath.length - 2; charIndex >= 0; charIndex--) {
    if (depPath[charIndex] === '(') {
      open--
    } else if (depPath[charIndex] === ')') {
      open++
    } else if (!open) {
      if (depPath.substring(charIndex + 1).startsWith('(patch_hash=')) {
        return {
          patchHashIndex: charIndex + 1,
          peersIndex: depPath.indexOf('(', charIndex + 2),
        }
      }
      return {
        patchHashIndex: -1,
        peersIndex: charIndex + 1,
      }
    }
  }
  return { peersIndex: -1, patchHashIndex: -1 }
}

export interface ParsedDepPath {
  id: string
  peerDepGraphHash: string
}

export function parseDepPath (relDepPath: string): ParsedDepPath {
  const { peersIndex } = indexOfDepPathSuffix(relDepPath)
  if (peersIndex !== -1) {
    return {
      id: relDepPath.substring(0, peersIndex),
      peerDepGraphHash: relDepPath.substring(peersIndex),
    }
  }
  return {
    id: relDepPath,
    peerDepGraphHash: '',
  }
}

export function removeSuffix (relDepPath: string): string {
  const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(relDepPath)
  if (patchHashIndex !== -1) {
    return relDepPath.substring(0, patchHashIndex)
  }
  if (peersIndex !== -1) {
    return relDepPath.substring(0, peersIndex)
  }
  return relDepPath
}

/**
 * Whether the package at `depPath` is itself patched. A patched peer nested in
 * the peers suffix does not count.
 */
export function hasPatchHash (depPath: string): boolean {
  return indexOfDepPathSuffix(depPath).patchHashIndex !== -1
}

export function removePeersSuffix (relDepPath: string): string {
  const { peersIndex } = indexOfDepPathSuffix(relDepPath)
  if (peersIndex !== -1) {
    return relDepPath.substring(0, peersIndex)
  }
  return relDepPath
}

export function getPkgIdWithPatchHash (depPath: DepPath): PkgIdWithPatchHash {
  return removePeersSuffix(depPath) as PkgIdWithPatchHash
}

export function tryGetPackageId (relDepPath: DepPath): PkgId {
  let pkgId: string = relDepPath
  const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(pkgId)
  const sepIndex = patchHashIndex === -1 ? peersIndex : patchHashIndex
  if (sepIndex !== -1) {
    pkgId = pkgId.substring(0, sepIndex)
  }
  if (pkgId.includes(':')) {
    const newPkgId = pkgId.substring(pkgId.indexOf('@', 1) + 1)
    // TODO: change the format of package ID to always start with the package name.
    // not only in the case of "runtime:" and registry-qualified ids
    if (!newPkgId.startsWith('runtime:') && parseRegistryQualifiedVersion(newPkgId) == null) {
      pkgId = newPkgId
    }
  }
  return pkgId as PkgId
}

export function getRegistryByPackageName (registriesByScope: RegistriesByScope, packageName: string): string {
  if (packageName[0] !== '@') return registriesByScope.default
  const scope = packageName.substring(0, packageName.indexOf('/'))
  return registriesByScope[scope] || registriesByScope.default
}

export function refToRelative (
  reference: string,
  pkgName: string
): DepPath | null {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference[0] === '@') return reference as DepPath
  const atIndex = reference.indexOf('@')
  if (atIndex === -1) return `${pkgName}@${reference}` as DepPath
  const colonIndex = reference.indexOf(':')
  const bracketIndex = reference.indexOf('(')
  if ((colonIndex === -1 || atIndex < colonIndex) && (bracketIndex === -1 || atIndex < bracketIndex)) return reference as DepPath
  return `${pkgName}@${reference}` as DepPath
}

export interface DependencyPath {
  name?: string
  peerDepGraphHash?: string
  version?: string
  nonSemverVersion?: PkgResolutionId
  patchHash?: string
  /** Set for registry-qualified dep paths (`<name>@<registryName>:<version>`); `version` then holds the bare semver part. */
  registryName?: string
}

export function parse (dependencyPath: string): DependencyPath {
  if (typeof dependencyPath !== 'string') {
    throw new TypeError(`Expected \`dependencyPath\` to be of type \`string\`, got \`${
      dependencyPath === null ? 'null' : typeof dependencyPath
    }\``)
  }
  const sepIndex = dependencyPath.indexOf('@', 1)
  if (sepIndex === -1) {
    return {}
  }
  const name = dependencyPath.substring(0, sepIndex)
  const rawVersion = dependencyPath.substring(sepIndex + 1)
  if (!rawVersion) {
    return {}
  }
  return buildParsedDependencyPath(name, rawVersion)
}

interface SplitSuffixResult {
  version: string
  peerDepGraphHash?: string
  patchHash?: string
}

function splitDepPathSuffixes (rawVersion: string): SplitSuffixResult {
  const { peersIndex, patchHashIndex } = indexOfDepPathSuffix(rawVersion)
  if (peersIndex === -1 && patchHashIndex === -1) {
    return { version: rawVersion }
  }
  if (peersIndex === -1) {
    return {
      version: rawVersion.substring(0, patchHashIndex),
      patchHash: rawVersion.substring(patchHashIndex),
    }
  }
  if (patchHashIndex === -1) {
    return {
      version: rawVersion.substring(0, peersIndex),
      peerDepGraphHash: rawVersion.substring(peersIndex),
    }
  }
  return {
    version: rawVersion.substring(0, patchHashIndex),
    patchHash: rawVersion.substring(patchHashIndex, peersIndex),
    peerDepGraphHash: rawVersion.substring(peersIndex),
  }
}

function buildParsedDependencyPath (name: string, rawVersion: string): DependencyPath {
  const { version, peerDepGraphHash, patchHash } = splitDepPathSuffixes(rawVersion)
  if (semver.valid(version)) {
    return { name, peerDepGraphHash, version, patchHash }
  }
  const registryQualified = parseRegistryQualifiedVersion(version)
  if (registryQualified != null) {
    return {
      name,
      peerDepGraphHash,
      version: registryQualified.version,
      patchHash,
      registryName: registryQualified.registryName,
    }
  }
  return {
    name,
    nonSemverVersion: version as PkgResolutionId,
    peerDepGraphHash,
    patchHash,
  }
}

export function depPathToFilename (depPath: string, maxLengthWithoutHash: number): string {
  let filename = depPathToFilenameUnescaped(depPath).replace(/[\\/:*?"<>|#]/g, '+')
  if (filename.includes('(')) {
    filename = filename
      .replace(/\)$/, '')
      .replace(/\)\(|\(|\)/g, '_')
  }
  // Windows strips trailing dots and spaces from path segments. Hashing
  // the unescaped name keeps it apart from a literal `+` path.
  const escapesAmbiguously = hasAmbiguousEscape(depPath)
  const hashInput = escapesAmbiguously ? depPath : filename
  let end = filename.length
  while (end > 0 && (filename[end - 1] === '.' || filename[end - 1] === ' ')) end--
  const escapedTrailing = end < filename.length
  if (escapedTrailing) {
    filename = filename.substring(0, end) + '+'.repeat(filename.length - end)
  }
  if (escapesAmbiguously || escapedTrailing || filename.length > maxLengthWithoutHash || filename !== filename.toLowerCase() && !filename.startsWith('file+')) {
    return `${filename.substring(0, maxLengthWithoutHash - 33)}_${createShortHash(hashInput)}`
  }
  return filename
}

/**
 * Whether escaping the URL or path of a non-registry dependency could map two
 * distinct dep paths to one name. Only `/` after the scheme escapes unambiguously.
 */
function hasAmbiguousEscape (depPath: string): boolean {
  const pkgId = depPath.split('(', 1)[0].replace(/^\//, '')
  const isFileDepPath = pkgId.startsWith('file:')
  const versionSeparator = pkgId.indexOf('@', 1)
  if (!isFileDepPath && versionSeparator === -1) return false
  const version = isFileDepPath ? pkgId : pkgId.slice(versionSeparator + 1)
  const schemeEnd = version.indexOf(':')
  if (schemeEnd === -1 || parseRegistryQualifiedVersion(version) != null) return false
  const location = version.slice(schemeEnd + 1).replace(/^\/\//, '')
  return /[+\\:*?"<>|#]/.test(location)
}

function depPathToFilenameUnescaped (depPath: string): string {
  if (!depPath.startsWith('file:')) {
    if (depPath[0] === '/') {
      depPath = depPath.substring(1)
    }
    const index = depPath.indexOf('@', 1)
    if (index === -1) return depPath
    return `${depPath.substring(0, index)}@${depPath.slice(index + 1)}`
  }
  return depPath.replace(':', '+')
}

// Peer ID or stringified peer dependency graph
export type PeerId = { name: string, version: string } | string

export function createPeerDepGraphHash (peerIds: PeerId[], maxLength: number = 1000): string {
  let dirName = peerIds.map(
    (peerId) => {
      if (typeof peerId !== 'string') {
        return `${peerId.name}@${peerId.version}`
      }
      if (peerId[0] === '/') {
        return peerId.substring(1)
      }
      return peerId
    }
  ).sort().join(')(')
  if (dirName.length > maxLength) {
    dirName = createShortHash(dirName)
  }
  return `(${dirName})`
}

const RUNTIME_DEP_PATH_RE = /^(?:node|bun|deno)@runtime:/

export function isRuntimeDepPath (depPath: DepPath): boolean {
  return RUNTIME_DEP_PATH_RE.test(depPath)
}

/**
 * The prefix of a lockfile reference to a directory inside the package that
 * declares it, such as `link:<root>/typings/css-tree`. `<root>` stands for
 * the declaring package's own directory, which is only known where that
 * package is placed on disk.
 */
export const PACKAGE_ROOT_LINK_PREFIX = 'link:<root>/'

/**
 * The path, relative to the declaring package's directory, that a
 * {@link PACKAGE_ROOT_LINK_PREFIX} reference points to, or `undefined` for
 * any other reference.
 *
 * The path comes from a lockfile or a published manifest, so it is accepted
 * only when every `/`-separated segment is a plain name: no empty, `.` or
 * `..` segment, and no `\` or `:`, which would let a platform path join
 * re-anchor it outside the package.
 */
export function packageRootLinkTarget (reference: string): string | undefined {
  if (!reference.startsWith(PACKAGE_ROOT_LINK_PREFIX)) return undefined
  const target = reference.slice(PACKAGE_ROOT_LINK_PREFIX.length)
  const isPlainPath = target.split('/').every((segment) =>
    segment !== '' && segment !== '.' && segment !== '..' && !segment.includes('\\') && !segment.includes(':')
  )
  return isPlainPath ? target : undefined
}
