import os from 'node:os'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import normalize from 'normalize-path'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const filespecPattern = isWindows ? /^(?:[./\\]|~\/|[a-z]:)/i : /^(?:[./]|~\/|[a-z]:)/i
const tarballFilenamePattern = /\.(?:tgz|tar\.gz|tar|tar\.bz2|tbz2|tbz)$/i
const isAbsolutePath = /^\/|^[A-Z]:/i
const driveLetterPrefixPattern = /^[a-z]:/i

export interface LocalPackageSpec {
  dependencyPath: string
  fetchSpec: string
  id: PkgResolutionId
  type: 'directory' | 'file'
  normalizedBareSpecifier: string
}

export interface WantedLocalDependency {
  bareSpecifier: string
  injected?: boolean
}

class PathIsUnsupportedProtocolError extends PnpmError {
  bareSpecifier: string
  protocol: string
  constructor (bareSpecifier: string, protocol: string) {
    super('PATH_IS_UNSUPPORTED_PROTOCOL', 'Local dependencies via `path:` protocol are not supported. ' +
      'Use the `link:` protocol for folder dependencies and `file:` for local tarballs')
    this.bareSpecifier = bareSpecifier
    this.protocol = protocol
  }
}

/**
 * Whether a bare specifier's shape can only mean a local file or directory:
 * the `link:` / `file:` protocols, a path-prefixed spec (`./`, `../`, `~/`,
 * absolute POSIX paths, and Windows drive paths — including drive-relative
 * ones like `C:dir`), or a bare tarball file name.
 *
 * Narrower than what {@link parseLocalPath} claims, which also takes any spec
 * containing a path separator. That shape is statically indistinguishable from
 * a hosted-git shorthand (`user/repo`) or a named-registry alias
 * (`gh:@scope/pkg`), and the resolver chain only gets away with claiming it by
 * running the local resolver last. Callers that dispatch on specifier shape
 * without that ordering ask this instead.
 */
export function isLocalFilesystemSpecifier (bareSpecifier: string): boolean {
  if (bareSpecifier.startsWith('link:') || bareSpecifier.startsWith('file:')) return true
  if (isFilespec(bareSpecifier)) return true
  // Any other protocol — a `git+ssh:` / `https:` URL, an `npm:` alias, a
  // named-registry prefix — belongs to its own resolver, tarball-shaped path
  // or not.
  if (bareSpecifier.includes(':')) return false
  // A `#` here marks a hosted-git shorthand's committish
  // (`user/repo#release.tgz`), not a local tarball: the protocol and
  // path-prefixed forms already returned above.
  if (bareSpecifier.includes('#')) return false
  return isTarballFilename(bareSpecifier)
}

/**
 * Whether a bare specifier's shape can only be a local path, so re-anchoring
 * it cannot change which resolver claims it.
 *
 * Stricter than {@link isLocalFilesystemSpecifier}, which answers a different
 * question: what a specifier *could* name, rather than which resolver reaches
 * it first. The chain runs the local path resolver last, so every shape that
 * is merely path-*like* has already been claimed by then — a slash-free
 * `repo.tgz` resolves as a dist-tag, and `user/repo.tgz` as a hosted-git
 * shorthand. Only a path-prefixed specifier lands on the local resolver, and
 * only those move.
 *
 * A `<letter>:` prefix is declined with them: a single-letter named registry
 * is well-formed, so `c:pkg@1` is a registry specifier as much as a drive
 * path. Nothing is lost by that — a drive path names the same place from
 * every directory, and a drive-relative one is measured from process state no
 * caller here can see.
 */
export function barePathIsUnambiguous (bareSpecifier: string): boolean {
  return !isDriveLetterPrefix(bareSpecifier) && isFilespec(bareSpecifier)
}

/**
 * Whether the spec opens with `<letter>:`, which reads as a Windows drive
 * path and as a single-letter named-registry alias alike.
 */
export function isDriveLetterPrefix (bareSpecifier: string): boolean {
  return driveLetterPrefixPattern.test(bareSpecifier)
}

/**
 * Whether a local specifier names a package tarball rather than a directory.
 * A `file:` specifier resolves to one or the other, and only the directory
 * form becomes a `link:` entry in the lockfile.
 */
export function isTarballFilename (bareSpecifier: string): boolean {
  return tarballFilenamePattern.test(bareSpecifier)
}

/**
 * Whether the spec is path-shaped:
 * - Windows: `/^(?:[./\\]|~\/|[a-z]:)/i`
 * - POSIX:   `/^(?:[./]|~\/|[a-z]:)/i`
 *
 * A path lacking that shape reads as a hosted-git shorthand instead.
 */
export function isFilespec (bareSpecifier: string): boolean {
  return filespecPattern.test(bareSpecifier)
}

export function parseLocalScheme (
  wd: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string,
  opts: { preserveAbsolutePaths: boolean }
): LocalPackageSpec | null {
  if (wd.bareSpecifier.startsWith('link:') || wd.bareSpecifier.startsWith('workspace:')) {
    return fromLocal(wd, projectDir, lockfileDir, 'directory', opts)
  }
  if (wd.bareSpecifier.startsWith('file:')) {
    const type = isTarballFilename(wd.bareSpecifier) ? 'file' : 'directory'
    return fromLocal(wd, projectDir, lockfileDir, type, opts)
  }
  if (wd.bareSpecifier.startsWith('path:')) {
    throw new PathIsUnsupportedProtocolError(wd.bareSpecifier, 'path:')
  }
  return null
}

export function parseLocalPath (
  wd: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string,
  opts: { preserveAbsolutePaths: boolean }
): LocalPackageSpec | null {
  if (isTarballFilename(wd.bareSpecifier) ||
    wd.bareSpecifier.includes(path.sep) ||
    isFilespec(wd.bareSpecifier)
  ) {
    const type = isTarballFilename(wd.bareSpecifier) ? 'file' : 'directory'
    return fromLocal(wd, projectDir, lockfileDir, type, opts)
  }
  return null
}

function fromLocal (
  { bareSpecifier, injected }: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string,
  type: 'file' | 'directory',
  opts: { preserveAbsolutePaths: boolean }
): LocalPackageSpec {
  const spec = bareSpecifier.replace(/\\/g, '/')
    .replace(/^(?:file|link|workspace):\/*([A-Z]:)/i, '$1') // drive name paths on windows
    .replace(/^(?:file|link|workspace):(?:\/*([~./]))?/, '$1')

  let protocol!: string
  if (bareSpecifier.startsWith('file:')) {
    protocol = 'file:'
  } else if (bareSpecifier.startsWith('link:')) {
    protocol = 'link:'
  } else {
    protocol = type === 'directory' && !injected ? 'link:' : 'file:'
  }
  let fetchSpec!: string
  let normalizedBareSpecifier!: string
  if (/^~\//.test(spec)) {
    // this is needed for windows and for file:~/foo/bar
    fetchSpec = resolvePath(os.homedir(), spec.slice(2))
    normalizedBareSpecifier = `${protocol}${spec}`
  } else {
    fetchSpec = resolvePath(projectDir, spec)
    if (isAbsolute(spec)) {
      normalizedBareSpecifier = `${protocol}${spec}`
    } else {
      normalizedBareSpecifier = `${protocol}${normalize(path.relative(projectDir, fetchSpec))}`
    }
  }

  function normalizeRelativeOrAbsolute (relativeTo: string, fromPath: string) {
    let specPath
    if (opts.preserveAbsolutePaths && isAbsolute(spec)) {
      specPath = path.resolve(fromPath)
    } else {
      specPath = path.relative(relativeTo, fromPath)
    }
    return normalize(specPath)
  }

  injected = protocol === 'file:'
  const dependencyPath = injected
    ? normalizeRelativeOrAbsolute(lockfileDir, fetchSpec)
    : normalize(path.resolve(fetchSpec))
  const id = (
    !injected && (type === 'directory' || projectDir === lockfileDir)
      ? `${protocol}${normalizeRelativeOrAbsolute(projectDir, fetchSpec)}`
      : `${protocol}${normalizeRelativeOrAbsolute(lockfileDir, fetchSpec)}`
  ) as PkgResolutionId

  return {
    dependencyPath,
    fetchSpec,
    id,
    normalizedBareSpecifier,
    type,
  }
}

function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return path.normalize(spec)
  return path.resolve(where, spec)
}

function isAbsolute (dir: string): boolean {
  if (dir[0] === '/') return true
  if (/^[A-Z]:/i.test(dir)) return true
  return false
}
