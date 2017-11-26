import osenv = require('osenv')
import path = require('path')

// tslint:disable-next-line
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/
const isFilename = /[.](?:tgz|tar.gz|tar)$/i
const isAbsolutePath = /^[/]|^[A-Za-z]:/

export interface LocalPackageSpec {
  fetchSpec: string,
  type: 'directory' | 'file',
  normalizedPref: string,
}

export default function parsePref (
  pref: string,
  where: string,
): LocalPackageSpec | null {
  if (pref.endsWith('.tgz')
    || pref.endsWith('.tar.gz')
    || pref.endsWith('.tar')
    || pref.includes(path.sep)
    || pref.startsWith('file:')
    || isFilespec.test(pref)) {
      return fromFile(pref, where)
    }
  return null
}

function fromFile (pref: string, where: string): LocalPackageSpec {
  if (!where) where = process.cwd()
  const type = isFilename.test(pref) ? 'file' : 'directory'

  const spec = pref.replace(/\\/g, '/')
    .replace(/^file:[/]*([A-Za-z]:)/, '$1') // drive name paths on windows
    .replace(/^file:(?:[/]*([~./]))?/, '$1')
  if (/^~[/]/.test(spec)) {
    // this is needed for windows and for file:~/foo/bar
    return {
      fetchSpec: resolvePath(osenv.home(), spec.slice(2)),
      normalizedPref: `file:${spec}`,
      type,
    }
  }
  const fetchSpec = resolvePath(where, spec)
  if (isAbsolute(spec)) {
    return {
      fetchSpec,
      normalizedPref: `file:${spec}`,
      type,
    }
  }
  return {
    fetchSpec,
    normalizedPref: `file:${path.relative(where, fetchSpec)}`,
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
