import { randomUUID } from 'node:crypto'
import { promises as fs } from 'node:fs'
import type { FileHandle } from 'node:fs/promises'
import path from 'node:path'

import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import { rimraf } from '@zkochan/rimraf'
import yaml from 'js-yaml'
import { isEmpty } from 'ramda'
import writeFileAtomic from 'write-file-atomic'

import { convertToLockfileFile, convertToLockfileObject } from './lockfileFormatConverters.js'
import { getWantedLockfileName } from './lockfileName.js'
import { lockfileLogger as logger } from './logger.js'
import { sortLockfileKeys } from './sortLockfileKeys.js'
import { ensureLockfileIsNotSymlink, extractEnvDocument, readLockfileToString, YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START } from './yamlDocuments.js'

const LOCKFILE_YAML_FORMAT = {
  blankLines: true,
  lineWidth: -1,
  noCompatMode: true,
  noRefs: true,
  sortKeys: false,
}

export function lockfileYamlDump (obj: object): string {
  return yaml.dump(obj, LOCKFILE_YAML_FORMAT)
}

/**
 * Returns the canonical post-write lockfile — structurally identical
 * to what `readWantedLockfile` would parse back. Lets callers like
 * the verification cache hash the as-saved form without re-reading.
 */
export async function writeWantedLockfile (
  pkgPath: string,
  wantedLockfile: LockfileObject,
  opts?: {
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    /** Pre-resolved filename; skips the `getWantedLockfileName` (and
     *  its `getCurrentBranch`) call when supplied. */
    lockfileName?: string
  }
): Promise<LockfileObject> {
  const wantedLockfileName: string = opts?.lockfileName ?? await getWantedLockfileName(opts)
  return writeLockfile(wantedLockfileName, pkgPath, wantedLockfile)
}

export async function writeCurrentLockfile (
  virtualStoreDir: string,
  currentLockfile: LockfileObject
): Promise<LockfileObject | undefined> {
  // empty lockfile is not saved
  if (isEmptyLockfile(currentLockfile)) {
    await rimraf(path.join(virtualStoreDir, 'lock.yaml'))
    return undefined
  }
  await fs.mkdir(virtualStoreDir, { recursive: true })
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile)
}

async function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: LockfileObject
): Promise<LockfileObject> {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  const lockfileToStringify = convertToLockfileFile(wantedLockfile)
  const yamlDoc = yamlStringify(lockfileToStringify)

  await writeLockfileDoc(lockfilePath, lockfileFilename, yamlDoc)

  // YAML drops undefined on serialize, so the in-memory LockfileFile
  // can carry fields (like an unset settings.dedupePeers) that won't
  // survive a round-trip; strip them to mirror what the next reader
  // will parse back.
  return convertToLockfileObject(stripUndefinedDeep(lockfileToStringify) as LockfileFile)
}

/**
 * Writes a serialized lockfile. Only `pnpm-lock.yaml` is led by an env
 * document; `lock.yaml` and git-branch lockfiles are written as-is.
 *
 * The env document is preserved by re-reading it. Ideally it would be captured
 * during the initial lockfile read and passed through to the write functions,
 * but that would require threading it through 25+ call sites. Re-reading is
 * cheap since the file is likely still in the OS page cache.
 *
 * A byte-identical rewrite is skipped. An up-to-date install — every
 * `--frozen-lockfile` run — would otherwise rewrite the lockfile for nothing,
 * and refuse a symlinked one over a write that changes no bytes
 * (https://github.com/pnpm/pnpm/issues/13073).
 */
async function writeLockfileDoc (lockfilePath: string, lockfileName: string, mainDoc: string): Promise<void> {
  if (lockfileName !== WANTED_LOCKFILE) {
    await writeFileAtomic(lockfilePath, mainDoc)
    return
  }
  const existing = await readLockfileToString(lockfilePath)
  const envDoc = existing == null ? null : extractEnvDocument(existing)
  const content = envDoc == null
    ? mainDoc
    : `${YAML_DOCUMENT_START}${envDoc}${YAML_DOCUMENT_SEPARATOR}${mainDoc}`
  if (existing === content) return
  await writeWantedLockfileAtomic(lockfilePath, content)
}

/**
 * Replaces `pnpm-lock.yaml` through a temp file and `rename`, carrying the
 * target's ownership and mode onto the replacement.
 *
 * `write-file-atomic` is not used here because it resolves the destination with
 * `realpath`, which a symlink swapped in after {@link ensureLockfileIsNotSymlink}
 * would follow. `rename` replaces the final path component itself and never
 * resolves it, so the write cannot be redirected.
 */
async function writeWantedLockfileAtomic (lockfilePath: string, content: string): Promise<void> {
  await ensureLockfileIsNotSymlink(lockfilePath)
  const targetStat = await fs.lstat(lockfilePath).catch((error: NodeJS.ErrnoException) => {
    if (error.code === 'ENOENT') return undefined
    throw error
  })
  const tempPath = path.join(
    path.dirname(lockfilePath),
    `.${path.basename(lockfilePath)}.${process.pid}.${randomUUID()}.tmp`
  )
  let tempFile: FileHandle | undefined
  try {
    tempFile = await fs.open(tempPath, 'wx', targetStat?.mode)
    await tempFile.writeFile(content)
    if (targetStat != null) {
      // The rename replaces the target with this fresh file, so an install
      // running as another user (root in a container over a bind-mounted repo)
      // would otherwise hand the lockfile's owner a file they can no longer
      // write. Ownership we aren't permitted to set is not fatal: the file just
      // stays ours, exactly as `write-file-atomic` treats it.
      await tempFile.chown(targetStat.uid, targetStat.gid).catch(ignoreUnprivilegedChown)
      await tempFile.chmod(targetStat.mode)
    }
    await tempFile.sync()
    await tempFile.close()
    tempFile = undefined

    // Check again at publication time. rename() replaces the final path entry
    // itself and never resolves it, so a swap after this check cannot redirect
    // the write through a symlink.
    await ensureLockfileIsNotSymlink(lockfilePath)
    await fs.rename(tempPath, lockfilePath)
  } finally {
    await tempFile?.close().catch(() => {})
    await fs.rm(tempPath, { force: true }).catch(() => {})
  }
}

/**
 * `chown` is refused for an unprivileged process that doesn't own the target,
 * and unimplemented on some filesystems. Neither is a reason to fail the write.
 */
function ignoreUnprivilegedChown (error: NodeJS.ErrnoException): void {
  const tolerated = error.code === 'ENOSYS' ||
    ((process.getuid == null || process.getuid() !== 0) && (error.code === 'EINVAL' || error.code === 'EPERM'))
  if (!tolerated) throw error
}

function stripUndefinedDeep<T> (value: T): T {
  if (value === null || typeof value !== 'object') return value
  if (Array.isArray(value)) return value.map(stripUndefinedDeep) as unknown as T
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (v === undefined) continue
    out[k] = stripUndefinedDeep(v)
  }
  return out as T
}

export function writeLockfileFile (
  lockfilePath: string,
  wantedLockfile: LockfileFile
): Promise<void> {
  const yamlDoc = yamlStringify(wantedLockfile)
  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: LockfileFile) {
  const sortedLockfile = sortLockfileKeys(lockfile as LockfileFile)
  return lockfileYamlDump(sortedLockfile)
}

export function isEmptyLockfile (lockfile: LockfileObject): boolean {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

export interface WriteLockfilesResult {
  /**
   * The canonical "as-saved" wanted lockfile — the inverse converter
   * applied to the same object that was serialized to YAML. Hashing
   * this is equivalent to hashing the lockfile the next install will
   * load from disk (modulo undefined values that YAML drops, which any
   * sensible canonicalization-then-hash routine should strip).
   */
  wantedLockfile: LockfileObject
  /** Same as above for the current lockfile, or undefined when it was skipped because empty. */
  currentLockfile: LockfileObject | undefined
}

export async function writeLockfiles (
  opts: {
    wantedLockfile: LockfileObject
    wantedLockfileDir: string
    currentLockfile: LockfileObject
    currentLockfileDir: string
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    /** See {@link writeWantedLockfile}'s `lockfileName` option. */
    wantedLockfileName?: string
  }
): Promise<WriteLockfilesResult> {
  const wantedLockfileName: string = opts.wantedLockfileName ?? await getWantedLockfileName(opts)
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, wantedLockfileName)
  const currentLockfilePath = path.join(opts.currentLockfileDir, 'lock.yaml')

  const wantedLockfileToStringify = convertToLockfileFile(opts.wantedLockfile)
  const yamlDoc = yamlStringify(wantedLockfileToStringify)

  // in most cases the `pnpm-lock.yaml` and `node_modules/.pnpm-lock.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (opts.wantedLockfile === opts.currentLockfile) {
    await Promise.all([
      writeLockfileDoc(wantedLockfilePath, wantedLockfileName, yamlDoc),
      (async () => {
        if (isEmptyLockfile(opts.wantedLockfile)) {
          await rimraf(currentLockfilePath)
        } else {
          await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
          // Current lockfile (node_modules/.pnpm/lock.yaml) does not include the env document
          await writeFileAtomic(currentLockfilePath, yamlDoc)
        }
      })(),
    ])
    // Both files share the same source object; strip once and reuse.
    const normalized = convertToLockfileObject(stripUndefinedDeep(wantedLockfileToStringify) as LockfileFile)
    return {
      wantedLockfile: normalized,
      currentLockfile: isEmptyLockfile(opts.wantedLockfile) ? undefined : normalized,
    }
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${path.relative(opts.wantedLockfileDir, currentLockfilePath)}\``,
    prefix: opts.wantedLockfileDir,
  })

  const currentLockfileToStringify = convertToLockfileFile(opts.currentLockfile)
  const currentYamlDoc = yamlStringify(currentLockfileToStringify)

  // Filtered-current callers (deps-restorer) can pass an empty
  // current against a non-empty wanted; key off the current.
  const currentIsEmpty = isEmptyLockfile(opts.currentLockfile)
  await Promise.all([
    writeLockfileDoc(wantedLockfilePath, wantedLockfileName, yamlDoc),
    (async () => {
      if (currentIsEmpty) {
        await rimraf(currentLockfilePath)
      } else {
        await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
        await writeFileAtomic(currentLockfilePath, currentYamlDoc)
      }
    })(),
  ])
  return {
    wantedLockfile: convertToLockfileObject(stripUndefinedDeep(wantedLockfileToStringify) as LockfileFile),
    currentLockfile: currentIsEmpty
      ? undefined
      : convertToLockfileObject(stripUndefinedDeep(currentLockfileToStringify) as LockfileFile),
  }
}
