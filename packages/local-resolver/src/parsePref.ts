import PnpmError from '@pnpm/error'
import os = require('os')
import normalize = require('normalize-path')
import path = require('path')

// eslint-disable-next-line
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/
const isFilename = /[.](?:tgz|tar.gz|tar)$/i
const isAbsolutePath = /^[/]|^[A-Za-z]:/

export interface LocalPackageSpec {
  dependencyPath: string
  fetchSpec: string
  id: string
  type: 'directory' | 'file'
  normalizedPref: string
}

export default function parsePref (
  pref: string,
  projectDir: string,
  lockfileDir: string
): LocalPackageSpec | null {
  if (pref.startsWith('link:') || pref.startsWith('workspace:')) {
    return fromLocal(pref, projectDir, lockfileDir, 'directory')
  }
  if (pref.endsWith('.tgz') ||
    pref.endsWith('.tar.gz') ||
    pref.endsWith('.tar') ||
    pref.includes(path.sep) ||
    pref.startsWith('file:') ||
    isFilespec.test(pref)
  ) {
    const type = isFilename.test(pref) ? 'file' : 'directory'
    return fromLocal(pref, projectDir, lockfileDir, type)
  }
  if (pref.startsWith('path:')) {
    const err = new PnpmError('PATH_IS_UNSUPPORTED_PROTOCOL', 'Local dependencies via `path:` protocol are not supported. ' +
      'Use the `link:` protocol for folder dependencies and `file:` for local tarballs')
    /* eslint-disable @typescript-eslint/dot-notation */
    err['pref'] = pref
    err['protocol'] = 'path:'
    /* eslint-enable @typescript-eslint/dot-notation */
    throw err
  }
  return null
}

function fromLocal (
  pref: string,
  projectDir: string,
  lockfileDir: string,
  type: 'file' | 'directory'
): LocalPackageSpec {
  const spec = pref.replace(/\\/g, '/')
    .replace(/^(file|link|workspace):[/]*([A-Za-z]:)/, '$2') // drive name paths on windows
    .replace(/^(file|link|workspace):(?:[/]*([~./]))?/, '$2')

  const protocol = type === 'directory' ? 'link:' : 'file:'
  let fetchSpec!: string
  let normalizedPref!: string
  if (/^~[/]/.test(spec)) {
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

  const dependencyPath = normalize(path.relative(projectDir, fetchSpec))
  const id = type === 'directory' || projectDir === lockfileDir
    ? `${protocol}${dependencyPath}`
    : `${protocol}${normalize(path.relative(lockfileDir, fetchSpec))}`

  return {
    dependencyPath,
    fetchSpec,
    id,
    normalizedPref,
    type,
  }
}

function resolvePath (where: string, spec: string) {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

function isAbsolute (dir: string) {
  if (dir[0] === '/') return true
  if (/^[A-Za-z]:/.test(dir)) return true
  return false
}
