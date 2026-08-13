import fs from 'node:fs'
import path from 'node:path'

/**
 * On-disk cache for per-version runtime `SHASUMS256.txt` bodies.
 *
 * A release's SHASUMS file lives under a version-pinned URL
 * (`.../v22.0.0/SHASUMS256.txt`), so its content is immutable: a body fetched
 * once — and, for signed channels, verified once — never needs to be fetched
 * again. Entries live under `<cacheDir>/v11/runtime-shasums/<host>/<url path>`
 * so clearing the cache directory clears them together with the registry
 * metadata mirror. The layout is shared with pacquet, which reads and writes
 * the same files.
 *
 * Only hand immutable URLs to this cache. A cached body is trusted on the
 * same terms as the registry metadata mirror: verification (the OpenPGP
 * signature check for signed channels) happens before the write, not after
 * the read, so a reader never re-verifies.
 */
export const RUNTIME_SHASUMS_DIR = 'v11/runtime-shasums'

/**
 * The cache file backing `url`, or `undefined` when the URL has a shape the
 * path mapping does not cover (non-HTTP scheme, embedded credentials, query
 * string, empty or dot-only path segments). Returning `undefined` just
 * disables caching for that URL.
 */
export function shasumsCachePath (cacheDir: string, url: string): string | undefined {
  let rest: string
  if (url.startsWith('https://')) {
    rest = url.substring('https://'.length)
  } else if (url.startsWith('http://')) {
    rest = url.substring('http://'.length)
  } else {
    return undefined
  }
  if (/[?#@]/.test(rest)) return undefined
  const firstSlash = rest.indexOf('/')
  if (firstSlash <= 0) return undefined
  // `:` (a port separator) is not portable in file names; `+` is the same
  // encoding the registry metadata mirror uses for it.
  const host = rest.substring(0, firstSlash).toLowerCase().replaceAll(':', '+')
  const parts = [encodePathSegment(host)]
  for (const segment of rest.substring(firstSlash + 1).split('/')) {
    if (!segment || segment === '.' || segment === '..') return undefined
    parts.push(encodePathSegment(segment))
  }
  if (parts.some((part) => part == null)) return undefined
  return path.join(cacheDir, RUNTIME_SHASUMS_DIR, ...(parts as string[]))
}

/**
 * Percent-encode the bytes of `segment` that are not portable across
 * filesystems, keeping `[A-Za-z0-9._+-]` as-is. pacquet applies the same
 * encoding, so both tools address one file.
 */
function encodePathSegment (segment: string): string | undefined {
  if (segment.length > 200) return undefined
  let encoded = ''
  for (const byte of Buffer.from(segment)) {
    const char = String.fromCharCode(byte)
    if (/[\w.+-]/.test(char)) {
      encoded += char
    } else {
      encoded += `%${byte.toString(16).toUpperCase().padStart(2, '0')}`
    }
  }
  return encoded
}

/**
 * The cached body for `url`, or `undefined` on any miss — a URL the mapping cannot represent,
 * a missing file, unreadable content, or an empty file (never a valid SHASUMS
 * body, so it only signals a torn write).
 */
export function readCachedShasums (cacheDir: string | undefined, url: string): string | undefined {
  if (cacheDir == null) return undefined
  const filePath = shasumsCachePath(cacheDir, url)
  if (filePath == null) return undefined
  let body: string
  try {
    body = fs.readFileSync(filePath, 'utf8')
  } catch {
    return undefined
  }
  return body || undefined
}

/**
 * Best-effort write of `body` for `url`: a cache-write failure only costs a
 * refetch on the next resolve, so errors are deliberately dropped rather than
 * failing the resolution that produced the body. The temp-file + rename keeps
 * concurrent writers (two installs resolving the same version) from exposing
 * a torn body.
 */
export function writeCachedShasums (cacheDir: string | undefined, url: string, body: string): void {
  if (cacheDir == null) return
  const filePath = shasumsCachePath(cacheDir, url)
  if (filePath == null) return
  const tempPath = `${filePath}.tmp${process.pid.toString()}`
  try {
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(tempPath, body)
    fs.renameSync(tempPath, filePath)
  } catch {
    try {
      fs.rmSync(tempPath, { force: true })
    } catch {}
  }
}
