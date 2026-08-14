import fs from 'node:fs'
import path from 'node:path'
import { threadId } from 'node:worker_threads'

/**
 * On-disk cache for per-version runtime `SHASUMS256.txt` bodies.
 *
 * A release's SHASUMS file lives under a version-pinned URL
 * (`.../v22.0.0/SHASUMS256.txt`), so its content is immutable: a body fetched
 * once — and, for signed channels, verified once — never needs to be fetched
 * again. Entries live under
 * `<cacheDir>/v11/runtime-shasums/<trust>/<host>/<url path>` so clearing the
 * cache directory clears them together with the registry metadata mirror. The
 * layout is shared with pacquet, which reads and writes the same files.
 *
 * Only hand immutable URLs to this cache. The cache directory is
 * *project-configurable* (`cacheDir`), so entries under it must never carry
 * more authority than the project that could have written them:
 *
 * - Signed-channel readers persist the detached signature next to the body
 *   and re-verify it against the embedded release keys on every read (see
 *   `fetchVerifiedNodeShasumsFileCached`), so a pre-seeded entry is only
 *   accepted if it is a genuine release body.
 * - Unverified entries are trusted on read, which grants a project nothing
 *   new: the unsigned channels' mirrors are already project-configurable
 *   (their bodies were project-controllable before the cache existed), and
 *   the musl list's download URLs are derived from the hardcoded
 *   unofficial-builds base, never from the cached body.
 *
 * The `<trust>` path segment keeps the two classes in disjoint subtrees so
 * their read policies cannot be confused.
 */
export const RUNTIME_SHASUMS_DIR = 'v11/runtime-shasums'

/**
 * How the body of a cache entry was authenticated before it was written.
 * Each class caches into its own subtree.
 */
export type ShasumsTrust = 'verified' | 'unverified'

export interface ShasumsCacheOpts {
  cacheDir?: string
  trust: ShasumsTrust
}

/**
 * Upper bound on a cache entry's size. Real SHASUMS bodies are a few
 * kilobytes; anything past this bound is not a release asset list and is
 * never read into memory or written.
 */
const MAX_CACHED_SHASUMS_LEN = 1024 * 1024

/**
 * The cached body for `url` as UTF-8 text, via {@link readCachedBytes}.
 * Invalid UTF-8 is a miss — a lossy decode would turn a corrupted entry
 * into malformed rows instead of a refetch.
 */
export async function readCachedShasums (url: string, opts: ShasumsCacheOpts): Promise<string | undefined> {
  const bytes = await readCachedBytes(url, opts)
  if (bytes == null) return undefined
  try {
    return strictUtf8Decoder.decode(bytes)
  } catch {
    return undefined
  }
}

const strictUtf8Decoder = new TextDecoder('utf8', { fatal: true })

/**
 * The cached bytes for `url`, or `undefined` on any miss — a URL the mapping
 * cannot represent, a missing or non-regular file, unreadable content, an
 * empty file (never a valid entry, so it only signals a torn write), or a
 * file over {@link MAX_CACHED_SHASUMS_LEN}.
 */
export async function readCachedBytes (url: string, opts: ShasumsCacheOpts): Promise<Buffer | undefined> {
  if (opts.cacheDir == null) return undefined
  const filePath = shasumsCachePath(opts.cacheDir, opts.trust, url)
  if (filePath == null) return undefined
  try {
    // A FIFO planted at the entry path would block a plain open until a
    // writer appears; O_NONBLOCK (a no-op for regular files, absent on
    // Windows, whose directory entries cannot be named pipes) makes the open
    // return immediately.
    const file = await fs.promises.open(filePath, fs.constants.O_RDONLY | (fs.constants.O_NONBLOCK ?? 0))
    try {
      // Checked on the opened handle, so nothing can swap the regular file
      // for a special one between check and read.
      if (!(await file.stat()).isFile()) return undefined
      // A bounded read rather than a stat check keeps the cap race-free: at
      // most one byte past the bound is ever read, whatever the file's size
      // becomes between open and read.
      const buffer = Buffer.allocUnsafe(MAX_CACHED_SHASUMS_LEN + 1)
      const { bytesRead } = await file.read(buffer, 0, buffer.length, 0)
      if (bytesRead === 0 || bytesRead > MAX_CACHED_SHASUMS_LEN) return undefined
      return buffer.subarray(0, bytesRead)
    } finally {
      await file.close()
    }
  } catch {
    return undefined
  }
}

/**
 * Best-effort write of `body` for `url`: a cache-write failure only costs a
 * refetch on the next resolve, so errors are deliberately dropped rather than
 * failing the resolution that produced the body. The exclusively-created temp
 * file + rename keeps concurrent writers (two installs resolving the same
 * version) from exposing a torn body.
 */
export async function writeCachedShasums (url: string, body: string | Uint8Array, opts: ShasumsCacheOpts): Promise<void> {
  if (opts.cacheDir == null) return
  if (Buffer.byteLength(body) > MAX_CACHED_SHASUMS_LEN) return
  const filePath = shasumsCachePath(opts.cacheDir, opts.trust, url)
  if (filePath == null) return
  // The process id alone does not make the temp name unique (worker threads
  // share it), so the thread id and a counter join it. The `wx` flag refuses
  // to open a path that already exists — a colliding writer or a pre-seeded
  // symlink fails the open instead of being followed — and any failure just
  // skips the write.
  const tempPath = `${filePath}.tmp-${process.pid.toString()}-${threadId.toString()}-${(tempCounter++).toString()}`
  try {
    await fs.promises.mkdir(path.dirname(filePath), { recursive: true })
    // Sync before the rename so a crash cannot persist the renamed name
    // pointing at partially-written content — a torn SHASUMS prefix still
    // parses and would otherwise be served (missing platform rows) until the
    // cache is cleared.
    const tempFile = await fs.promises.open(tempPath, 'wx')
    try {
      await tempFile.writeFile(body)
      await tempFile.sync()
    } finally {
      await tempFile.close()
    }
    await fs.promises.rename(tempPath, filePath)
  } catch {
    try {
      await fs.promises.rm(tempPath, { force: true })
    } catch {}
  }
}

let tempCounter = 0

/**
 * The cache file backing `url`, or `undefined` when the URL has a shape the
 * path mapping does not cover (non-HTTP scheme, embedded credentials, query
 * string, empty or dot-only path segments). Returning `undefined` just
 * disables caching for that URL.
 */
function shasumsCachePath (cacheDir: string, trust: ShasumsTrust, url: string): string | undefined {
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
  return path.join(cacheDir, RUNTIME_SHASUMS_DIR, trust, ...(parts as string[]))
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
