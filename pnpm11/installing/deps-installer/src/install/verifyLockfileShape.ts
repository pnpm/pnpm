import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

import { isGitHostedTarballUrl } from '@pnpm/lockfile.utils'
import { isDriveLetterPrefix, isTarballFilename } from '@pnpm/resolving.local-resolver'
import normalize from 'normalize-path'

import { isNonRelativeTarget, normalizeRelDir, stripFileUrlPrefix } from './verifyLockfileLocalPolicy.js'

const REMOTE_GIT_SCHEMES = new Set(['http', 'https', 'ssh', 'git', 'rsync', 'ftp', 'ftps'])

export interface FileShapeContext {
  allowDeployFileUrlKeys?: boolean
  lockfileDir?: string
  skipFileKeyTargetCheck?: boolean
}

export function hasLocalFilesystemResolution (resolution: unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const { type, tarball, variants } = resolution as { type?: unknown, tarball?: unknown, variants?: unknown }
  if (type === 'directory') return true
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.some((variant) => hasLocalFilesystemResolution((variant as { resolution?: unknown })?.resolution))
  }
  return typeof tarball === 'string' && stripFileUrlPrefix(tarball) != null
}

export function isPurelyLocalFilesystemResolution (resolution: unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const { type, tarball, variants } = resolution as { type?: unknown, tarball?: unknown, variants?: unknown }
  if (type === 'directory') return true
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.length > 0 && variants.every((variant) => isPurelyLocalFilesystemResolution((variant as { resolution?: unknown })?.resolution))
  }
  return typeof tarball === 'string' && stripFileUrlPrefix(tarball) != null
}

export function isLocalGitRepo (repo: string): boolean {
  const rest = stripGitPlusPrefix(repo.trim())
  if (!rest) return false
  if (stripFileUrlPrefix(rest) != null) return true
  const schemeIdx = rest.indexOf('://')
  if (schemeIdx >= 0) {
    return !REMOTE_GIT_SCHEMES.has(rest.slice(0, schemeIdx).toLowerCase())
  }
  if (isDriveLetterPrefix(rest)) return true
  const colonIdx = rest.indexOf(':')
  if (colonIdx >= 2) {
    const head = rest.slice(0, colonIdx)
    if (!head.includes('/') && !head.includes('\\')) return false
  }
  return true
}

export function extractLocalGitTarget (raw: string): string | undefined {
  if (!isLocalGitRepo(raw)) return undefined
  const withoutGit = stripGitPlusPrefix(raw.trim())
  const beforeFragment = withoutGit.split('#')[0] ?? withoutGit
  const rawPath = stripFileUrlPrefix(beforeFragment) ?? beforeFragment
  return rawPath.startsWith('//') ? decodeFileUrlOrSlashPath(rawPath) : canonicalSlashPath(rawPath)
}

export function localGitKeyRawPath (nonSemverVersion: string | undefined): string | undefined {
  if (nonSemverVersion == null || nonSemverVersion.slice(0, 9).toLowerCase() !== 'git+file:') return undefined
  const rest = nonSemverVersion.slice(9)
  return rest.split('#')[0] ?? rest
}

export function isLocalGitKey (nonSemverVersion: string | undefined): boolean {
  return localGitKeyRawPath(nonSemverVersion) != null
}

export function hasLocalGitResolution (resolution: unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const { type, repo, variants } = resolution as { type?: unknown, repo?: unknown, variants?: unknown }
  if (type === 'git' && typeof repo === 'string') return isLocalGitRepo(repo)
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.some((variant) => hasLocalGitResolution((variant as { resolution?: unknown })?.resolution))
  }
  return false
}

export function isLocalGitPackage (nonSemverVersion: string | undefined, resolution: unknown): boolean {
  return isLocalGitKey(nonSemverVersion) || hasLocalGitResolution(resolution)
}

export function decodeFileUrlOrSlashPath (keyPath: string): string {
  if (!keyPath.startsWith('//')) return canonicalSlashPath(keyPath)
  try {
    const decoded = decodeURIComponent(keyPath)
    const hostPath = stripLocalFileAuthority(decoded)
    if (hostPath == null) return canonicalSlashPath(decoded)
    return canonicalSlashPath(isDriveLetterPrefix(hostPath) ? hostPath : `/${hostPath}`)
  } catch {
    return canonicalSlashPath(keyPath)
  }
}

function stripLocalFileAuthority (decoded: string): string | undefined {
  if (decoded.startsWith('///')) return decoded.slice(3)
  const localhost = '//localhost/'
  return decoded.slice(0, localhost.length).toLowerCase() === localhost ? decoded.slice(localhost.length) : undefined
}

export function canonicalSlashPath (raw: string): string {
  const slash = raw.replace(/\\/g, '/')
  if (isDriveLetterPrefix(slash)) {
    const drive = slash[0].toUpperCase()
    const afterDrive = slash.slice(2)
    // `C:dir` resolves against the drive's current directory, so it keeps a
    // prefix distinct from the anchored `C:/dir`.
    if (!afterDrive.startsWith('/')) return `${drive}:${normalizeRelDir(afterDrive)}`
    const rest = normalizeRelDir(afterDrive.replace(/^\/+/, ''))
    return rest === '.' ? `${drive}:/` : `${drive}:/${rest}`
  }
  if (slash.startsWith('//') && !slash.startsWith('///')) {
    return `//${normalizeRelDir(slash.slice(2))}`
  }
  if (slash.startsWith('/')) {
    return normalizeRelDir(slash)
  }
  return normalizeRelDir(slash)
}

export function isDeployFileUrlKey (nonSemverVersion: string | undefined, directory: string): boolean {
  if (!nonSemverVersion?.startsWith('file:')) return false
  const filePath = nonSemverVersion.slice(5)
  return !isTarballFilename(filePath) && (filePath.startsWith('///') || (filePath.startsWith('//') && filePath !== directory))
}

export function fileKeyMismatchesLocalTarget (
  nonSemverVersion: string,
  resolution: unknown,
  ctx?: FileShapeContext
): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const keyPath = nonSemverVersion.slice(5)
  const { type, directory, tarball, variants } = resolution as {
    type?: unknown
    directory?: unknown
    tarball?: unknown
    variants?: unknown
  }
  if (type === 'directory' && typeof directory === 'string') {
    return directoryTargetMismatches(keyPath, directory, ctx)
  }
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.some((variant) => fileKeyMismatchesLocalTarget(
      nonSemverVersion,
      (variant as { resolution?: unknown })?.resolution,
      ctx
    ))
  }
  if (type === 'git') return true
  return typeof tarball === 'string' && tarballTargetMismatches(keyPath, tarball, ctx)
}

export function localGitKeyMismatchesTarget (
  keyPath: string,
  resolution: unknown,
  ctx?: FileShapeContext
): boolean {
  if (resolution == null || typeof resolution !== 'object') return true
  const { type, repo, variants } = resolution as { type?: unknown, repo?: unknown, variants?: unknown }
  if (type === 'git' && typeof repo === 'string') return gitRepoMismatchesKey(keyPath, repo, ctx)
  if (type === 'variations' && Array.isArray(variants)) {
    return variants.some((variant) => localGitKeyMismatchesTarget(
      keyPath,
      (variant as { resolution?: unknown })?.resolution,
      ctx
    ))
  }
  return true
}

function gitRepoMismatchesKey (keyPath: string, repo: string, ctx?: FileShapeContext): boolean {
  const repoTarget = extractLocalGitTarget(repo)
  if (repoTarget == null) return true
  if (!keyPath.startsWith('//')) {
    return normalizeRelDir(keyPath) !== normalizeRelDir(repoTarget)
  }
  return !fileUrlMatchesTarget(keyPath, repoTarget, ctx)
}

function directoryTargetMismatches (
  keyPath: string,
  directory: string,
  ctx?: FileShapeContext
): boolean {
  if (!keyPath.startsWith('//')) {
    return normalizeRelDir(keyPath) !== normalizeRelDir(directory)
  }
  if (isTarballFilename(keyPath)) return true
  return !fileUrlMatchesTarget(keyPath, directory, ctx)
}

function tarballTargetMismatches (
  keyPath: string,
  tarball: string,
  ctx?: FileShapeContext
): boolean {
  const tarPath = stripFileUrlPrefix(tarball)
  // A remote tarball has its own URL key, so one behind a `file:` key
  // disguises its source.
  if (tarPath == null) return true
  if (!keyPath.startsWith('//')) {
    return normalizeRelDir(keyPath) !== normalizeRelDir(tarPath)
  }
  return !fileUrlMatchesTarget(keyPath, tarPath, ctx)
}

function fileUrlMatchesTarget (keyPath: string, target: string, ctx?: FileShapeContext): boolean {
  if (ctx?.lockfileDir != null) {
    return resolvedFileUrlMatchesTarget(keyPath, target, ctx.lockfileDir)
  }
  if (isNonRelativeTarget(target)) {
    return absoluteFileUrlMatchesTarget(keyPath, target)
  }
  return Boolean(ctx?.allowDeployFileUrlKeys) &&
    (keyPath.startsWith('///') || (keyPath.startsWith('//') && keyPath !== target))
}

function absoluteFileUrlMatchesTarget (keyPath: string, target: string): boolean {
  try {
    return sameNormalizedPath(url.fileURLToPath(`file:${keyPath}`), target)
  } catch {
    return decodeFileUrlOrSlashPath(keyPath) === canonicalSlashPath(target)
  }
}

function resolvedFileUrlMatchesTarget (keyPath: string, target: string, lockfileDir: string): boolean {
  let fromUrl: string
  try {
    fromUrl = url.fileURLToPath(`file:${keyPath}`)
  } catch {
    // A drive-less file URL has no path on Windows.
    return decodeFileUrlOrSlashPath(keyPath) === canonicalSlashPath(target)
  }
  const fromTarget = path.resolve(lockfileDir, target)
  if (sameNormalizedPath(fromUrl, fromTarget)) return true
  try {
    return sameNormalizedPath(fs.realpathSync.native(fromUrl), fs.realpathSync.native(fromTarget))
  } catch {
    return false
  }
}

function sameNormalizedPath (left: string, right: string): boolean {
  return canonicalizeDriveLetter(normalize(path.resolve(left))) === canonicalizeDriveLetter(normalize(path.resolve(right)))
}

function canonicalizeDriveLetter (filePath: string): string {
  return isDriveLetterPrefix(filePath) ? `${filePath[0].toUpperCase()}${filePath.slice(1)}` : filePath
}

export function shapeMismatchReason (
  resolution: unknown,
  nonSemverVersion: string | undefined,
  ctx?: FileShapeContext
): string | undefined {
  if (nonSemverVersion == null && !isRegistryShapedResolution(resolution)) {
    return 'a registry-style dependency path is backed by a non-registry resolution'
  }
  if (!nonSemverVersion?.startsWith('file:') && hasLocalFilesystemResolution(resolution)) {
    return 'a non-file dependency path is backed by a local filesystem resolution'
  }
  if (!ctx?.skipFileKeyTargetCheck && nonSemverVersion?.startsWith('file:') && fileKeyMismatchesLocalTarget(nonSemverVersion, resolution, ctx)) {
    return 'a file dependency path does not match its local resolution target'
  }
  if (!isLocalGitKey(nonSemverVersion) && hasLocalGitResolution(resolution)) {
    return 'a non-local-git dependency path is backed by a local git resolution'
  }
  const gitRaw = localGitKeyRawPath(nonSemverVersion)
  if (!ctx?.skipFileKeyTargetCheck && gitRaw != null && localGitKeyMismatchesTarget(gitRaw, resolution, ctx)) {
    return 'a local git dependency path does not match its git repository target'
  }
  return undefined
}

export function isRegistryOriginResolution (resolution: unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  const { type, tarball, integrity } = resolution as { type?: unknown, tarball?: unknown, integrity?: unknown }
  return type == null && tarball == null && typeof integrity === 'string' && integrity.length > 0
}

function isRegistryShapedResolution (resolution: unknown): boolean {
  if (resolution == null) return true
  if (typeof resolution !== 'object') return false
  const { type, gitHosted, tarball, variants } = resolution as {
    type?: unknown
    gitHosted?: unknown
    tarball?: unknown
    variants?: unknown
  }
  if (type === 'variations') return areRegistryShapedVariants(variants)
  if (typeof type === 'string' && type.startsWith('custom:')) return true
  if (type != null) return false
  return isRegistryShapedTarballResolution(gitHosted, tarball)
}

function areRegistryShapedVariants (variants: unknown): boolean {
  return Array.isArray(variants) && variants.length > 0 && variants.every((variant) => {
    const resolution = (variant as { resolution?: unknown })?.resolution
    return resolution != null && isRegistryShapedResolution(resolution)
  })
}

function isRegistryShapedTarballResolution (gitHosted: unknown, tarball: unknown): boolean {
  if (gitHosted != null && (typeof gitHosted !== 'boolean' || gitHosted)) return false
  if (typeof tarball !== 'string' || tarball === '') return true
  return /^https?:\/\//i.test(tarball) && !isGitHostedTarballUrl(tarball)
}

function stripGitPlusPrefix (input: string): string {
  return input.slice(0, 4).toLowerCase() === 'git+' ? input.slice(4) : input
}
