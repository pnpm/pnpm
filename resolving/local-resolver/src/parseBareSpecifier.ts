import os from 'os'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import normalize from 'normalize-path'
import { type PkgResolutionId } from '@pnpm/resolver-base'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[./\\]|~\/|[a-z]:)/i : /^(?:[./]|~\/|[a-z]:)/i
const isFilename = /\.(?:tgz|tar.gz|tar)$/i
const isAbsolutePath = /^\/|^[A-Z]:/i

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

export function parseBareSpecifier (
  wd: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string,
  opts: { preserveAbsolutePaths: boolean }
): LocalPackageSpec | null {
  if (wd.bareSpecifier.startsWith('link:') || wd.bareSpecifier.startsWith('workspace:')) {
    return fromLocal(wd, projectDir, lockfileDir, 'directory', opts)
  }
  if (wd.bareSpecifier.endsWith('.tgz') ||
    wd.bareSpecifier.endsWith('.tar.gz') ||
    wd.bareSpecifier.endsWith('.tar') ||
    wd.bareSpecifier.includes(path.sep) ||
    wd.bareSpecifier.startsWith('file:') ||
    isFilespec.test(wd.bareSpecifier)
  ) {
    const type = isFilename.test(wd.bareSpecifier) ? 'file' : 'directory'
    return fromLocal(wd, projectDir, lockfileDir, type, opts)
  }
  if (wd.bareSpecifier.startsWith('path:')) {
    throw new PathIsUnsupportedProtocolError(wd.bareSpecifier, 'path:')
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
      normalizedBareSpecifier = `${protocol}${path.relative(projectDir, fetchSpec)}`
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
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

function isAbsolute (dir: string): boolean {
  if (dir[0] === '/') return true
  if (/^[A-Z]:/i.test(dir)) return true
  return false
}
