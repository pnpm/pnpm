import fs, { promises as fsp } from 'node:fs'
import path from 'node:path'

import {
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { isError, PnpmError } from '@pnpm/error'
import { mergeLockfileChanges } from '@pnpm/lockfile.merger'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId } from '@pnpm/types'
import { comverToSemver } from 'comver-to-semver'
import yaml from 'js-yaml'
import semver from 'semver'
import stripBom from 'strip-bom'

import { LockfileBreakingChangeError } from './errors/index.js'
import { getGitBranchLockfileNames } from './gitBranchLockfile.js'
import { autofixMergeConflicts, isDiff } from './gitMergeFile.js'
import { convertToLockfileFile, convertToLockfileObject } from './lockfileFormatConverters.js'
import { selectWantedLockfile } from './lockfileName.js'
import { lockfileLogger as logger } from './logger.js'
import { extractMainDocument } from './yamlDocuments.js'

export async function readCurrentLockfile (
  pnpmInternalDir: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
  }
): Promise<LockfileObject | null> {
  const lockfilePath = path.join(pnpmInternalDir, 'lock.yaml')
  return (await _read(lockfilePath, pnpmInternalDir, opts)).lockfile
}

export async function readWantedLockfileAndAutofixConflicts (
  pkgPath: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<{
  lockfile: LockfileObject | null
  hadConflicts: boolean
}> {
  return _readWantedLockfile(pkgPath, {
    ...opts,
    autofixMergeConflicts: true,
  })
}

/**
 * {@link readWantedLockfile} plus what the caller cannot reconstruct from
 * the returned lockfile: whether autofixing a merge conflict rewrote it,
 * and, under `mergeGitBranchLockfiles`, the importers as they stood
 * before the branch lockfiles were folded in. Telling a merged entry from
 * one the read file already carried needs that "before", which the merged
 * object no longer holds.
 */
export async function readWantedLockfileWithMergeInfo (
  pkgPath: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    autofixMergeConflicts?: boolean
  }
): Promise<{
  lockfile: LockfileObject | null
  hadConflicts: boolean
  preMergeImporters: LockfileObject['importers'] | undefined
}> {
  return _readWantedLockfile(pkgPath, opts)
}

export async function readWantedLockfile (
  pkgPath: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<LockfileObject | null> {
  return (await _readWantedLockfile(pkgPath, opts)).lockfile
}

/**
 * Read the wanted lockfile in its on-disk shape ({@link LockfileFile}),
 * skipping the conversion to the in-memory {@link LockfileObject}.
 *
 * Use this when the caller needs the exact serialized form — e.g. to
 * forward the lockfile to a server that speaks the on-disk format —
 * rather than the in-process representation.
 */
export async function readWantedLockfileFile (
  pkgPath: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<LockfileFile | null> {
  return (await _readWantedLockfile(pkgPath, opts)).lockfileFile
}

export function wantedLockfileHasMergeConflictsSync (pkgPath: string, lockfileName: string = WANTED_LOCKFILE): boolean {
  try {
    const lockfileRawContent = stripBom(fs.readFileSync(path.join(pkgPath, lockfileName), 'utf8'))
    return isDiff(extractMainDocument(lockfileRawContent))
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return false
    }
    throw err
  }
}

interface ReadLockfileResult {
  lockfile: LockfileObject | null
  /** The lockfile in its on-disk shape, before {@link convertToLockfileObject}. */
  lockfileFile: LockfileFile | null
  hadConflicts: boolean
}

interface ParsedLockfile {
  lockfile: LockfileObject
  lockfileFile: LockfileFile
  hadConflicts: boolean
}

const NO_LOCKFILE: ReadLockfileResult = { lockfile: null, lockfileFile: null, hadConflicts: false }

async function _read (
  lockfilePath: string,
  prefix: string, // only for logging
  opts: {
    autofixMergeConflicts?: boolean
    wantedVersions?: string[]
    ignoreIncompatible: boolean
  }
): Promise<ReadLockfileResult> {
  const lockfileRawContent = await readLockfileMainDocument(lockfilePath)
  if (lockfileRawContent == null) {
    return { ...NO_LOCKFILE }
  }
  const parsed = parseLockfile(lockfileRawContent, { lockfilePath, prefix, autofixMergeConflicts: opts.autofixMergeConflicts })
  if (isCompatibleLockfileVersion(parsed.lockfile, { wantedVersions: opts.wantedVersions, prefix })) {
    return parsed
  }
  if (opts.ignoreIncompatible) {
    logger.warn({
      message: `Ignoring not compatible lockfile at ${lockfilePath}`,
      prefix,
    })
    return { ...NO_LOCKFILE }
  }
  throw new LockfileBreakingChangeError(lockfilePath, {
    lockfileVersion: parsed.lockfile.lockfileVersion?.toString(),
    wantedVersions: opts.wantedVersions,
  })
}

/** The lockfile's content without the env document in front of it, or `null` when there is none. */
async function readLockfileMainDocument (lockfilePath: string): Promise<string | null> {
  let lockfileRawContent
  try {
    lockfileRawContent = stripBom(await fsp.readFile(lockfilePath, 'utf8'))
  } catch (err: unknown) {
    if (!(isError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
    return null
  }
  lockfileRawContent = extractMainDocument(lockfileRawContent)
  return lockfileRawContent.trim() ? lockfileRawContent : null
}

function parseLockfile (
  lockfileRawContent: string,
  opts: {
    lockfilePath: string
    prefix: string
    autofixMergeConflicts?: boolean
  }
): ParsedLockfile {
  try {
    const lockfileFile = yaml.load(lockfileRawContent) as LockfileFile
    return { lockfileFile, lockfile: convertToLockfileObject(lockfileFile), hadConflicts: false }
  } catch (err: unknown) {
    if (!opts.autofixMergeConflicts || !isDiff(lockfileRawContent)) {
      throw new PnpmError('BROKEN_LOCKFILE', `The lockfile at "${opts.lockfilePath}" is broken: ${formatLockfileError(err)}`)
    }
  }
  const lockfile = autofixMergeConflicts(lockfileRawContent)
  const lockfileFile = convertToLockfileFile(lockfile)
  logger.info({
    message: `Merge conflict detected in ${WANTED_LOCKFILE} and successfully merged`,
    prefix: opts.prefix,
  })
  return { lockfile, lockfileFile, hadConflicts: true }
}

function isCompatibleLockfileVersion (
  lockfile: LockfileObject,
  opts: {
    wantedVersions?: string[]
    prefix: string
  }
): boolean {
  const lockfileSemver = comverToSemver((lockfile.lockfileVersion ?? 0).toString())
  if (!opts.wantedVersions || opts.wantedVersions.length === 0) return true
  return opts.wantedVersions.some((wantedVersion) => {
    if (semver.major(lockfileSemver) !== semver.major(comverToSemver(wantedVersion))) return false
    if (lockfile.lockfileVersion !== '6.1' && semver.gt(lockfileSemver, comverToSemver(wantedVersion))) {
      logger.warn({
        message: `Your ${WANTED_LOCKFILE} was generated by a newer version of pnpm. ` +
          `It is a compatible version but it might get downgraded to version ${wantedVersion}`,
        prefix: opts.prefix,
      })
    }
    return true
  })
}

function formatLockfileError (err: unknown): string {
  if (isYamlException(err)) {
    const reason = typeof err.reason === 'string' ? err.reason : 'Unable to parse YAML'
    const line = err.mark?.line
    const column = err.mark?.column
    const position = typeof line === 'number' && Number.isFinite(line) &&
      typeof column === 'number' && Number.isFinite(column)
      ? ` (${line + 1}:${column + 1})`
      : ''
    return `${reason}${position}`
  }
  return isError(err) ? err.message : String(err)
}

function isYamlException (err: unknown): err is YamlExceptionLike {
  return typeof err === 'object' && err !== null &&
    'name' in err && err.name === 'YAMLException'
}

interface YamlExceptionLike {
  name: 'YAMLException'
  reason?: unknown
  mark?: {
    line?: unknown
    column?: unknown
  } | null
}

export function createLockfileObject (
  importerIds: ProjectId[],
  opts: {
    lockfileVersion: string
    autoInstallPeers: boolean
    excludeLinksFromLockfile: boolean
    peersSuffixMaxLength: number
  }
): LockfileObject {
  const importers: LockfileObject['importers'] = {}
  for (const importerId of importerIds) {
    importers[importerId] = {
      dependencies: {},
      specifiers: {},
    }
  }
  return {
    importers,
    lockfileVersion: opts.lockfileVersion || LOCKFILE_VERSION,
    settings: {
      autoInstallPeers: opts.autoInstallPeers,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
    },
  }
}

async function _readWantedLockfile (
  pkgPath: string,
  opts: {
    wantedVersions?: string[]
    ignoreIncompatible: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    autofixMergeConflicts?: boolean
  }
): Promise<{
  lockfile: LockfileObject | null
  lockfileFile: LockfileFile | null
  hadConflicts: boolean
  preMergeImporters: LockfileObject['importers'] | undefined
}> {
  const lockfileNames = await getWantedLockfileCandidates(opts)
  let result: ReadLockfileResult = { ...NO_LOCKFILE }
  let preMergeImporters: LockfileObject['importers'] | undefined
  /* eslint-disable no-await-in-loop -- the first lockfile candidate that exists wins, so later ones are not read */
  for (const lockfileName of lockfileNames) {
    result = await _read(path.join(pkgPath, lockfileName), pkgPath, { ...opts, autofixMergeConflicts: true })
    if (!result.lockfile) continue
    if (opts.mergeGitBranchLockfiles) {
      preMergeImporters = result.lockfile.importers
      result.lockfile = await _mergeGitBranchLockfiles(result.lockfile, pkgPath, pkgPath, opts)
      result.lockfileFile = result.lockfile ? convertToLockfileFile(result.lockfile) : null
    }
    break
  }
  /* eslint-enable no-await-in-loop */
  return { ...result, preMergeImporters }
}

async function getWantedLockfileCandidates (opts: { useGitBranchLockfile?: boolean }): Promise<string[]> {
  const lockfileNames: string[] = [WANTED_LOCKFILE]
  if (opts.useGitBranchLockfile) {
    const { fileName, detachedHeadCandidates } = await selectWantedLockfile(opts)
    if (fileName !== WANTED_LOCKFILE) {
      lockfileNames.unshift(fileName)
    }
    lockfileNames.unshift(...detachedHeadCandidates)
  }
  return lockfileNames
}

async function _mergeGitBranchLockfiles (
  lockfile: LockfileObject | null,
  lockfileDir: string,
  prefix: string,
  opts: {
    autofixMergeConflicts?: boolean
    wantedVersions?: string[]
    ignoreIncompatible: boolean
  }
): Promise<LockfileObject | null> {
  if (!lockfile) {
    return lockfile
  }
  const gitBranchLockfiles: Array<(LockfileObject | null)> = (await _readGitBranchLockfiles(lockfileDir, prefix, opts)).map(({ lockfile }) => lockfile)

  let mergedLockfile: LockfileObject = lockfile

  for (const gitBranchLockfile of gitBranchLockfiles) {
    if (!gitBranchLockfile) {
      continue
    }
    mergedLockfile = mergeLockfileChanges(mergedLockfile, gitBranchLockfile)
  }

  return mergedLockfile
}

async function _readGitBranchLockfiles (
  lockfileDir: string,
  prefix: string,
  opts: {
    autofixMergeConflicts?: boolean
    wantedVersions?: string[]
    ignoreIncompatible: boolean
  }
): Promise<Array<{
  lockfile: LockfileObject | null
  hadConflicts: boolean
}>> {
  const files = await getGitBranchLockfileNames(lockfileDir)

  return Promise.all(files.map((file) => _read(path.join(lockfileDir, file), prefix, opts)))
}
