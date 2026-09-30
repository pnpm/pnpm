import type fs from 'node:fs'

// Whether a file with these `stats` may have been modified at or after
// `referenceMs`. On a filesystem that keeps sub-second mtimes the
// comparison is exact; on one that rounds mtimes down to whole seconds
// (ext4 with 128-byte inodes, HFS+, some CI runner disks) the file could
// have been touched anywhere within its second, so the whole second
// counts as possibly-after. Without this a manifest, lockfile, patch, or
// pnpmfile edited in the same second as the previous install looks
// unchanged and the deps-status fast path wrongly reports up to date.
// Erring toward "modified" only runs the authoritative content check; it
// never skips a needed install, and it is a no-op on sub-second
// filesystems, so the common case is unaffected.
//
// `mtimeMs` keeps the sub-millisecond fraction, so a whole-second mtime
// has no remainder modulo 1000; `mtime.valueOf()` is truncated to whole
// milliseconds and would misread a sub-millisecond mtime as whole-second.
export function modifiedAtOrAfter (stats: fs.Stats, referenceMs: number): boolean {
  const wholeSecond = stats.mtimeMs % 1000 === 0
  const mtimeMs = stats.mtime.valueOf()
  return wholeSecond ? mtimeMs + 1000 > referenceMs : mtimeMs > referenceMs
}
