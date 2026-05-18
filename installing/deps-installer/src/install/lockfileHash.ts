import { hashObject } from '@pnpm/crypto.object-hasher'
import { convertToLockfileFile, convertToLockfileObject, type LockfileObject } from '@pnpm/lockfile.fs'

/**
 * Cache-stable hash of an in-memory lockfile.
 *
 * The verification cache stores the hash post-resolution and then
 * compares it against a hash computed from the lockfile parsed back
 * off disk on the next install. Two transformations stand between the
 * in-memory write object and the load-parsed object:
 *
 * - {@link convertToLockfileFile} / {@link convertToLockfileObject}
 *   shape-shift between `packages` and `(packages, snapshots)`. Running
 *   both produces the canonical load shape from any in-memory shape.
 * - The YAML round-trip naturally drops keys whose values are
 *   `undefined`. The converters don't, so the in-memory object can
 *   carry leftover settings (e.g. `settings.dedupePeers = undefined`)
 *   that disappear once written and re-read. `object-hash` treats
 *   `undefined` vs absent as distinct, so we strip undefined values
 *   here to match what the on-disk YAML produces.
 */
export function hashLockfile (lockfile: LockfileObject): string {
  return hashObject(stripUndefined(convertToLockfileObject(convertToLockfileFile(lockfile))))
}

function stripUndefined (value: unknown): unknown {
  if (value === null || typeof value !== 'object') return value
  if (Array.isArray(value)) return value.map(stripUndefined)
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (v === undefined) continue
    out[k] = stripUndefined(v)
  }
  return out
}
