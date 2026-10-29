import type { FileSignature, LocalDependencies, Validation } from './types.js'
import { fileDigestOf, type RecordedLockfile, type RecordedValidation } from './validation.js'

/** The local dependencies the lockfile of a `RecordedLockfile` resolves. */
export type ReadLocalDependencies = (lockfile: RecordedLockfile) => Promise<LocalDependencies | undefined>

/** The digests of the lockfiles the projects in `validation` were proven against. */
export function recordedLockfileDigests (validation: Validation): string[] {
  return [validation.lockfile, ...Object.values(validation.lockfiles ?? {})].filter((digest): digest is string => digest != null)
}

interface LocalDependencyRecords {
  entries: Record<string, LocalDependencies>
  signatures: Record<string, FileSignature>
}

/**
 * Records in `validation` the local dependencies of each lockfile the write
 * vouched for projects against, keyed by its digest: the entry `previous`
 * records for that digest, or the ones `read` finds in the lockfile. Carries
 * from `recorded.carried` the entries of the other lockfiles its projects
 * still name, with the signatures they were recorded with.
 */
export async function recordLocalDependencies (
  validation: Validation,
  recorded: RecordedValidation,
  opts: { previous: Validation | undefined, read: ReadLocalDependencies | undefined }
): Promise<void> {
  const referenced = new Set(recordedLockfileDigests(validation))
  const records: LocalDependencyRecords = { entries: {}, signatures: {} }
  await recordVouchedLockfiles(records, recorded.lockfiles, { ...opts, referenced })
  carryLockfiles(records, recorded.carried, referenced)
  // An empty entry stays, so that the next write of the same lockfile reads it from here.
  if (Object.keys(records.entries).length === 0) return
  validation.localDependencies = records.entries
  validation.files = { ...validation.files, ...records.signatures }
}

async function recordVouchedLockfiles (
  records: LocalDependencyRecords,
  lockfiles: Map<string, RecordedLockfile>,
  opts: { previous: Validation | undefined, read: ReadLocalDependencies | undefined, referenced: Set<string> }
): Promise<void> {
  for (const [digest, lockfile] of lockfiles) {
    if (!opts.referenced.has(digest)) continue
    // eslint-disable-next-line no-await-in-loop -- a write records one lockfile, or one per project with its own
    const local = opts.previous?.localDependencies?.[digest] ?? await opts.read?.(lockfile)
    if (local == null) continue
    records.entries[digest] = local
    for (const tarball of local.tarballs ?? []) {
      const digestOfTarball = fileDigestOf(opts.previous, tarball)
      if (digestOfTarball.kind === 'present') records.signatures[tarball] = digestOfTarball.signature
    }
  }
}

function carryLockfiles (
  records: LocalDependencyRecords,
  carried: Validation | undefined,
  referenced: Set<string>
): void {
  for (const [digest, local] of Object.entries(carried?.localDependencies ?? {})) {
    if (!referenced.has(digest) || records.entries[digest] != null) continue
    records.entries[digest] = local
    for (const tarball of local.tarballs ?? []) {
      const signature = carried?.files?.[tarball]
      if (signature != null) records.signatures[tarball] = signature
    }
  }
}
