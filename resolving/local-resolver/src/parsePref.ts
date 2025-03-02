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
  normalizedPref: string
}

export interface WantedLocalDependency {
  pref: string
  injected?: boolean
}

export function parsePref (
  wd: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string
): LocalPackageSpec | null {
  if (wd.pref.startsWith('link:') || wd.pref.startsWith('workspace:')) {
    return fromLocal(wd, projectDir, lockfileDir, 'directory')
  }
  if (wd.pref.endsWith('.tgz') ||
    wd.pref.endsWith('.tar.gz') ||
    wd.pref.endsWith('.tar') ||
    wd.pref.includes(path.sep) ||
    wd.pref.startsWith('file:') ||
    isFilespec.test(wd.pref)
  ) {
    const type = isFilename.test(wd.pref) ? 'file' : 'directory'
    return fromLocal(wd, projectDir, lockfileDir, type)
  }
  if (wd.pref.startsWith('path:')) {
    const err = new PnpmError('PATH_IS_UNSUPPORTED_PROTOCOL', 'Local dependencies via `path:` protocol are not supported. ' +
      'Use the `link:` protocol for folder dependencies and `file:` for local tarballs')
    // @ts-expect-error
    err['pref'] = wd.pref
    // @ts-expect-error
    err['protocol'] = 'path:'

    throw err
  }
  return null
}

function fromLocal (
  { pref, injected }: WantedLocalDependency,
  projectDir: string,
  lockfileDir: string,
  type: 'file' | 'directory'
): LocalPackageSpec {
  const spec = pref.replace(/\\/g, '/')
    .replace(/^(?:file|link|workspace):\/*([A-Z]:)/i, '$1') // drive name paths on windows
    .replace(/^(?:file|link|workspace):(?:\/*([~./]))?/, '$1')

  let protocol!: string
  if (pref.startsWith('file:')) {
    protocol = 'file:'
  } else if (pref.startsWith('link:')) {
    protocol = 'link:'
  } else {
    protocol = type === 'directory' && !injected ? 'link:' : 'file:'
  }
  let fetchSpec!: string
  let normalizedPref!: string
  if (/^~\//.test(spec)) {
    // this is needed for windows and for file:~/foo/bar
    fetchSpec = resolvePath(os.homedir(), spec.slice(2))
    normalizedPref = `${protocol}${spec}`
  } else {
    fetchSpec = resolvePath(projectDir, spec)
    if (isAbsolute(spec)) {
      normalizedPref = `${protocol}${spec}`
    } else {
      normalizedPref = `${protocol}${path.relative(projectDir, fetchSpec)}`
    }
  }

  injected = protocol === 'file:'
  const dependencyPath = injected
    ? normalize(path.relative(lockfileDir, fetchSpec))
    : normalize(path.resolve(fetchSpec))
  const id = (
    !injected && (type === 'directory' || projectDir === lockfileDir)
      ? `${protocol}${normalize(path.relative(projectDir, fetchSpec))}`
      : `${protocol}${normalize(path.relative(lockfileDir, fetchSpec))}`
  ) as PkgResolutionId

  return {
    dependencyPath,
    fetchSpec,
    id,
    normalizedPref,
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
