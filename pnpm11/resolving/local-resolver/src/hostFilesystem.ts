import { barePathIsUnambiguous, isDriveLetterPrefix } from './parseBareSpecifier.js'

/**
 * Whether `specifier`, declared by an importer at POSIX-relative `importerDir`
 * (`.` for the workspace root), resolves against the host filesystem outside an
 * in-memory workspace package map.
 */
export function isHostFilesystemSpecifier (
  specifier: string,
  importerDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): boolean {
  if (hasLocalFileProtocol(specifier) || stripPackageNameFromLocalSpec(specifier) != null) return true
  const workspaceSuffix = stripAsciiCasePrefix(specifier, 'workspace:')
  if (workspaceSuffix != null) {
    return workspaceSpecEscapesRoot(workspaceSuffix, importerDir)
  }
  const npmAlias = stripAsciiCasePrefix(specifier, 'npm:')
  if (npmAlias != null) {
    return npmAliasWrapsLocalProtocol(npmAlias)
  }
  if (isDriveLetterPrefix(specifier)) {
    const afterColon = specifier.slice(2)
    return afterColon.startsWith('/') || afterColon.startsWith('\\') || !isDeclaredPrefix(specifier.slice(0, 1))
  }
  if (isUnambiguousBarePath(specifier)) return true
  return !specifier.includes(':') && isUnprefixedLocalPath(specifier)
}

/**
 * Whether `specifier` declared in a non-local package manifest targets the host
 * filesystem or a path-based `workspace:` dependency.
 *
 * Assumes valid package-internal `file:` directory paths have already been
 * rewritten to `link:<root>/...` and filtered out by the caller.
 */
export function isNonLocalFilesystemSubdepSpec (
  specifier: string,
  isDeclaredPrefix: (prefix: string) => boolean
): boolean {
  const peeled = peelNpmAlias(specifier)
  if (hasNonLocalForbiddenProtocol(peeled) || stripPackageNameFromLocalSpec(peeled) != null) return true
  const workspaceSuffix = stripAsciiCasePrefix(peeled, 'workspace:')
  if (workspaceSuffix != null) {
    return isWorkspaceFilesystemSuffix(workspaceSuffix, isDeclaredPrefix)
  }
  if (isDriveLetterPrefix(peeled)) {
    const afterColon = peeled.slice(2)
    return afterColon.startsWith('/') || afterColon.startsWith('\\') || !isDeclaredPrefix(peeled.slice(0, 1))
  }
  if (barePathIsUnambiguous(peeled) || isUnambiguousBarePath(peeled)) return true
  return !peeled.includes(':') && isUnprefixedLocalPath(peeled)
}

/**
 * Whether `relPath`, interpreted relative to POSIX `importerDir` (`.` at the
 * workspace root), escapes outside the workspace root.
 */
export function escapesWorkspaceRoot (importerDir: string, relPath: string): boolean {
  if (isTildePath(relPath) || isRootedOrWindowsPath(relPath)) return true
  const initialDepth = importerDepth(importerDir)
  return initialDepth < 0 || climbsAboveDepth(relPath, initialDepth)
}

/**
 * The local specifier that follows the `<name>@` of a specifier written as a
 * package key, such as the `lib@file:///workspace/lib` that `pnpm deploy`
 * writes into the deployed manifest. `undefined` unless `specifier` is a
 * package name, `@`, and a `file:`, `link:`, `path:`, or `git+file:` specifier.
 */
export function stripPackageNameFromLocalSpec (specifier: string): string | undefined {
  const atIndex = specifier.indexOf('@', specifier.startsWith('@') ? 1 : 0)
  if (atIndex < 1) return undefined
  const name = specifier.slice(0, atIndex)
  const slashCount = name.split('/').length - 1
  const isPackageName = slashCount === (name.startsWith('@') ? 1 : 0) &&
    !name.endsWith('/') &&
    !/^[._~]/.test(name) &&
    !name.includes(':') &&
    !name.includes('\\')
  const rest = specifier.slice(atIndex + 1)
  return isPackageName && hasNonLocalForbiddenProtocol(rest) ? rest : undefined
}

function peelNpmAlias (specifier: string): string {
  let current = specifier
  for (let depth = 0; depth < 8; depth++) {
    const aliased = stripAsciiCasePrefix(current, 'npm:')
    if (aliased == null) break
    const atIndex = aliased.lastIndexOf('@')
    if (atIndex < 1) break
    current = aliased.slice(atIndex + 1)
  }
  return current
}

function hasNonLocalForbiddenProtocol (specifier: string): boolean {
  return hasLocalFileProtocol(specifier) ||
    stripAsciiCasePrefix(specifier, 'path:') != null
}

function hasLocalFileProtocol (specifier: string): boolean {
  return stripAsciiCasePrefix(specifier, 'file:') != null ||
    stripAsciiCasePrefix(specifier, 'link:') != null ||
    stripAsciiCasePrefix(specifier, 'git+file:') != null
}

function npmAliasWrapsLocalProtocol (aliased: string): boolean {
  const atIndex = aliased.lastIndexOf('@')
  return atIndex >= 1 && hasLocalFileProtocol(aliased.slice(atIndex + 1))
}

function isTildeDirPrefix (value: string): boolean {
  return value.startsWith('~/') || value.startsWith('~\\')
}

function isTildePath (value: string): boolean {
  return value === '~' || isTildeDirPrefix(value)
}

function isWorkspaceFilesystemSuffix (suffix: string, isDeclaredPrefix: (prefix: string) => boolean): boolean {
  if (isTildeDirPrefix(suffix) || suffix.startsWith('.') || isRootedOrWindowsPath(suffix)) {
    return true
  }
  const aliasVersion = splitWorkspaceAliasVersion(suffix)
  return aliasVersion != null && aliasVersion !== '~' && isNonLocalFilesystemSubdepSpec(aliasVersion, isDeclaredPrefix)
}

function splitWorkspaceAliasVersion (suffix: string): string | undefined {
  if (suffix.length === 0 || suffix.startsWith('.') || suffix.startsWith('_') || suffix.startsWith('/')) {
    return undefined
  }
  const atIndex = suffix.indexOf('@', 1)
  return atIndex === -1 ? undefined : suffix.slice(atIndex + 1)
}

function workspaceSpecEscapesRoot (suffix: string, importerDir: string): boolean {
  if (isTildeDirPrefix(suffix) || suffix.includes('\\') || isDriveLetterPrefix(suffix)) {
    return true
  }
  return (suffix.startsWith('.') || suffix.startsWith('/')) && escapesWorkspaceRoot(importerDir, suffix)
}

function isRootedOrWindowsPath (relPath: string): boolean {
  return relPath.startsWith('/') || relPath.includes('\\') || isDriveLetterPrefix(relPath)
}

function climbsAboveDepth (relPath: string, initialDepth: number): boolean {
  let depth = initialDepth
  for (const segment of relPath.split('/')) {
    const nextDepth = applyPathSegment(depth, segment)
    if (nextDepth < 0) return true
    depth = nextDepth
  }
  return false
}

function applyPathSegment (depth: number, segment: string): number {
  if (segment === '' || segment === '.') return depth
  if (segment === '..') return depth - 1
  return depth + 1
}

function importerDepth (importerDir: string): number {
  if (isTildePath(importerDir) || isRootedOrWindowsPath(importerDir)) return -1
  let depth = 0
  for (const segment of importerDir.split('/')) {
    depth = applyPathSegment(depth, segment)
    if (depth < 0) return -1
  }
  return depth
}

function isUnambiguousBarePath (specifier: string): boolean {
  return specifier.startsWith('.') ||
    specifier.startsWith('/') ||
    specifier.startsWith('\\') ||
    isTildeDirPrefix(specifier)
}

function isUnprefixedLocalPath (specifier: string): boolean {
  const hashIndex = specifier.indexOf('#')
  const pathPart = hashIndex === -1 ? specifier : specifier.slice(0, hashIndex)
  if (pathPart.includes('\\')) return true
  if (!pathPart.includes('/') || pathPart.startsWith('@')) return false
  let segments = 0
  for (const segment of pathPart.split('/')) {
    if (segment === '' || segment === '.' || segment === '..') return true
    segments += 1
  }
  return segments >= 3
}

function stripAsciiCasePrefix (specifier: string, prefix: string): string | undefined {
  return specifier.length >= prefix.length && specifier.slice(0, prefix.length).toLowerCase() === prefix
    ? specifier.slice(prefix.length)
    : undefined
}
