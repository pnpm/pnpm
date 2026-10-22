import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { equals } from 'ramda'

import { type FileSignature, type ProjectsList, type Validation, WORKSPACE_STATE_SETTING_KEYS, type WorkspaceState, type WorkspaceStateSettings } from './types.js'

/** How many hex digits of a SHA-256 digest a record keeps. */
const DIGEST_LENGTH = 32

/**
 * A filesystem that keeps whole seconds may round an mtime down by up to this
 * much (FAT rounds to two seconds).
 */
const WHOLE_SECOND_GRANULARITY_NS = 2_000_000_000

/**
 * The digest of a manifest. The value is hashed the way pnpm v12 hashes it, so
 * a manifest that only reorders its keys or its whitespace keeps its digest,
 * and either CLI recognizes what the other recorded.
 */
export function manifestDigest (manifest: unknown): string {
  const hash = crypto.createHash('sha256')
  hashValue(hash, manifest)
  return hash.digest('hex').slice(0, DIGEST_LENGTH)
}

/**
 * Feeds `value` into `hash` unambiguously: each node is tagged by its kind,
 * each string and container by its length, and object keys in the order of
 * their UTF-8 bytes.
 */
function hashValue (hash: crypto.Hash, value: unknown): void {
  if (Array.isArray(value)) {
    hashArray(hash, value)
  } else if (typeof value === 'object' && value !== null) {
    hashObject(hash, value as Record<string, unknown>)
  } else {
    hashScalar(hash, value)
  }
}

function hashScalar (hash: crypto.Hash, value: unknown): void {
  switch (typeof value) {
    case 'boolean':
      hash.update(value ? 't' : 'f')
      return
    case 'number':
      hash.update('#')
      hashString(hash, String(value))
      return
    case 'string':
      hash.update('s')
      hashString(hash, value)
      return
    default:
      hash.update('n')
  }
}

function hashArray (hash: crypto.Hash, items: unknown[]): void {
  hash.update('[')
  hashLength(hash, items.length)
  for (const item of items) hashValue(hash, item)
}

function hashObject (hash: crypto.Hash, object: Record<string, unknown>): void {
  const entries = Object.entries(object)
    .filter(([, item]) => item !== undefined)
    .map(([key, item]) => [Buffer.from(key), item] as const)
    .sort(([left], [right]) => Buffer.compare(left, right))
  hash.update('{')
  hashLength(hash, entries.length)
  for (const [key, item] of entries) {
    hashLength(hash, key.length)
    hash.update(key)
    hashValue(hash, item)
  }
}

function hashString (hash: crypto.Hash, text: string): void {
  const bytes = Buffer.from(text)
  hashLength(hash, bytes.length)
  hash.update(bytes)
}

function hashLength (hash: crypto.Hash, length: number): void {
  const bytes = Buffer.alloc(8)
  bytes.writeBigUInt64LE(BigInt(length))
  hash.update(bytes)
}

/** The lockfile's importer id for the project at `rootDir`. */
export function importerIdOf (workspaceDir: string, rootDir: string): string {
  const relative = path.relative(workspaceDir, rootDir).split(path.sep).join('/')
  return relative === '' ? '.' : relative
}

/** What the project with `importerId` was proven installed with. */
export function projectValidation (
  validation: Validation,
  importerId: string
): { manifest: string, lockfile: string | undefined } | undefined {
  const manifest = validation.manifests[importerId]
  if (manifest == null) return undefined
  const exception = validation.lockfiles?.[importerId]
  return { manifest, lockfile: exception === undefined ? validation.lockfile : exception ?? undefined }
}

export type FileDigest =
  | { kind: 'absent' }
  /** The file exists but could not be read, so nothing about it is proven. */
  | { kind: 'unreadable' }
  | { kind: 'present', signature: FileSignature }

/**
 * The digest of the file at `filePath`, reusing the one `recorded` holds
 * wherever the file's stat proves its content did not change.
 */
export function fileDigestOf (recorded: Validation | undefined, filePath: string): FileDigest {
  const stat = statFile(filePath)
  if (stat == null) return { kind: 'absent' }
  if (stat === 'unreadable') return { kind: 'unreadable' }
  const signature = trustedSignature(recorded, filePath)
  if (signature != null && statMatches(stat, signature)) return { kind: 'present', signature }
  try {
    const digest = crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex').slice(0, DIGEST_LENGTH)
    return { kind: 'present', signature: { ...stat.signature, digest } }
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return { kind: 'absent' }
    return { kind: 'unreadable' }
  }
}

/** Whether the next check trusts the stat of every file `validation` records. */
export function trustsEveryFile (validation: Validation): boolean {
  return Object.values(validation.files ?? {}).every((signature) => signatureTrusted(signature, validation.recordedAtNs))
}

/** Whether the signature `recorded` holds for `filePath` stands for its content. */
export function fileSignatureTrusted (recorded: Validation | undefined, filePath: string): boolean {
  const signature = trustedSignature(recorded, filePath)
  const stat = statFile(filePath)
  return signature != null && stat != null && stat !== 'unreadable' && statMatches(stat, signature)
}

/**
 * The signature `recorded` holds for `filePath`, when the file was last
 * written before the recording stat'd it. One written in the same filesystem
 * clock tick, or within the rounding of a whole-second filesystem, could have
 * changed again without its stat moving.
 */
function trustedSignature (recorded: Validation | undefined, filePath: string): FileSignature | undefined {
  const signature = recorded?.files?.[filePath]
  if (signature == null) return undefined
  return signatureTrusted(signature, recorded!.recordedAtNs) ? signature : undefined
}

function signatureTrusted (signature: FileSignature, recordedAtNs: number): boolean {
  const changedNs = Math.max(signature.mtimeNs, signature.ctimeNs ?? signature.mtimeNs)
  const window = changedNs % 1_000_000_000 === 0 ? WHOLE_SECOND_GRANULARITY_NS : 0
  return changedNs + window < recordedAtNs
}

interface FileStat {
  signature: Omit<FileSignature, 'digest'>
}

function statFile (filePath: string): FileStat | 'unreadable' | undefined {
  let stats: fs.BigIntStats
  try {
    stats = fs.statSync(filePath, { bigint: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return undefined
    return 'unreadable'
  }
  if (!stats.isFile()) return 'unreadable'
  return {
    signature: {
      mtimeNs: Number(stats.mtimeNs),
      ctimeNs: Number(stats.ctimeNs),
      size: Number(stats.size),
    },
  }
}

function statMatches ({ signature }: FileStat, recorded: FileSignature): boolean {
  return signature.mtimeNs === recorded.mtimeNs &&
    signature.ctimeNs === recorded.ctimeNs &&
    signature.size === recorded.size
}

/** The lockfile the project records name, and its digest. */
export function lockfileDigest (
  recorded: Validation | undefined,
  lockfileDir: string,
  wantedLockfileName: string
): { path: string, digest: FileDigest } {
  const wanted = path.join(lockfileDir, wantedLockfileName)
  const digest = fileDigestOf(recorded, wanted)
  if (digest.kind !== 'absent') return { path: wanted, digest }
  const current = path.join(lockfileDir, 'node_modules/.pnpm/lock.yaml')
  return { path: current, digest: fileDigestOf(recorded, current) }
}

/**
 * The patch files and the pnpmfiles every project shares, as recorded.
 * `pnpmfiles` is unset when `--ignore-pnpmfile` skipped loading them.
 */
export function sharedInputPaths (
  lockfileDir: string,
  patchedDependencies: Record<string, string> | undefined,
  pnpmfiles: string[] | undefined
): string[] {
  return [
    ...Object.values(patchedDependencies ?? {}).map((patchFile) => path.resolve(lockfileDir, patchFile)),
    ...pnpmfiles ?? [],
  ]
}

export interface RecordValidationOptions {
  allProjects: ProjectsList
  workspaceDir: string
  lockfileDir: string
  wantedLockfileName: string
  /** The patch files and the pnpmfiles every project shares. */
  sharedInputs: string[]
  /**
   * The root directories of the projects this write vouches for, or
   * `undefined` for every project. The other projects keep the record
   * `previous` holds for them.
   */
  vouchedProjects?: Set<string>
  previous?: WorkspaceState
  sharedWorkspaceLockfile?: boolean
}

/**
 * Records in `state` what this write proves, and marks it filtered when a
 * project is left unproven. The files are stat'd before the filesystem clock
 * is probed, so a file rewritten after its stat either has a stat that no
 * longer matches or falls inside the window `fileDigestOf` distrusts.
 */
export function recordValidation (state: WorkspaceState, opts: RecordValidationOptions): void {
  const previous = opts.vouchedProjects == null ? undefined : opts.previous
  const { lockfile, projectLockfiles, files } = recordedFiles(opts, previous?.validation)
  const recordedAtNs = filesystemNowNs(path.join(opts.workspaceDir, 'node_modules'))
  if (recordedAtNs == null) {
    delete state.validation
    state.filteredInstall = true
    return
  }
  const carried = previous != null && sharedInputsMatch(previous, state, opts.sharedInputs, files) ? previous.validation : undefined
  const validation: Validation = { recordedAtNs, files, lockfile, manifests: {} }
  const recorded = recordProjects({ validation, opts, carried, projectLockfiles })
  state.lastValidatedTimestamp = Math.trunc(recordedAtNs / 1_000_000)
  state.filteredInstall = recorded !== opts.allProjects.length
  state.validation = validation
}

/**
 * The lockfile digest and the signatures of the shared input files, read the
 * way the next check reads them.
 */
function recordedFiles (
  opts: RecordValidationOptions,
  previous: Validation | undefined
): {
  lockfile: string | undefined
  projectLockfiles: Map<string, string | undefined> | undefined
  files: Record<string, FileSignature>
} {
  const files: Record<string, FileSignature> = {}
  for (const input of opts.sharedInputs) {
    const digest = fileDigestOf(previous, input)
    if (digest.kind === 'present') files[input] = digest.signature
  }
  if (opts.sharedWorkspaceLockfile === false) {
    return {
      lockfile: undefined,
      projectLockfiles: recordedProjectLockfiles(opts, previous, files),
      files,
    }
  }
  const lockfile = lockfileDigest(previous, opts.lockfileDir, opts.wantedLockfileName)
  if (lockfile.digest.kind === 'present') files[lockfile.path] = lockfile.digest.signature
  return {
    lockfile: lockfile.digest.kind === 'present' ? lockfile.digest.signature.digest : undefined,
    projectLockfiles: undefined,
    files,
  }
}

function recordedProjectLockfiles (
  opts: RecordValidationOptions,
  previous: Validation | undefined,
  files: Record<string, FileSignature>
): Map<string, string | undefined> {
  const projectLockfiles = new Map<string, string | undefined>()
  for (const project of opts.allProjects) {
    if (opts.vouchedProjects != null && !opts.vouchedProjects.has(project.rootDir)) continue
    const wantedPath = path.join(project.rootDir, opts.wantedLockfileName)
    const digest = fileDigestOf(previous, wantedPath)
    if (digest.kind === 'present') {
      files[wantedPath] = digest.signature
      projectLockfiles.set(project.rootDir, digest.signature.digest)
    } else if (digest.kind !== 'unreadable') {
      projectLockfiles.set(project.rootDir, undefined)
    }
  }
  return projectLockfiles
}

interface ProjectRecordContext {
  validation: Validation
  opts: RecordValidationOptions
  carried: Validation | undefined
  projectLockfiles: Map<string, string | undefined> | undefined
}

/**
 * Records in `validation` each project the write vouches for, proven against
 * its lockfile, and each other one `carried` still proves. Returns how many it
 * recorded.
 */
function recordProjects (ctx: ProjectRecordContext): number {
  const { opts, validation } = ctx
  let recorded = 0
  for (const project of opts.allProjects) {
    const importerId = importerIdOf(opts.lockfileDir, project.rootDir)
    const record = recordProjectEntry(ctx, project, importerId)
    if (record == null) continue
    insertProject(validation, importerId, record)
    recorded++
  }
  return recorded
}

function recordProjectEntry (
  ctx: ProjectRecordContext,
  project: RecordValidationOptions['allProjects'][number],
  importerId: string
): { manifest: string, lockfile: string | undefined } | undefined {
  const { opts, projectLockfiles, validation } = ctx
  const vouched = opts.vouchedProjects == null || opts.vouchedProjects.has(project.rootDir)
  if (!vouched) {
    return carriedProjectEntry(ctx, project.rootDir, importerId)
  }
  if (projectLockfiles != null && !projectLockfiles.has(project.rootDir)) return undefined
  return {
    manifest: manifestDigest(project.manifest),
    lockfile: projectLockfiles != null ? projectLockfiles.get(project.rootDir) : validation.lockfile,
  }
}

function carriedProjectEntry (
  { carried, opts, projectLockfiles, validation }: ProjectRecordContext,
  projectRootDir: string,
  importerId: string
): { manifest: string, lockfile: string | undefined } | undefined {
  if (carried == null) return undefined
  const record = projectValidation(carried, importerId)
  if (record != null && projectLockfiles != null && validation.files != null) {
    const wantedPath = path.join(projectRootDir, opts.wantedLockfileName)
    const carriedSig = trustedSignature(carried, wantedPath)
    if (carriedSig != null) validation.files[wantedPath] = carriedSig
  }
  return record
}

function insertProject (
  validation: Validation,
  importerId: string,
  { manifest, lockfile }: { manifest: string, lockfile: string | undefined }
): void {
  validation.manifests[importerId] = manifest
  if (lockfile !== validation.lockfile) {
    validation.lockfiles ??= {}
    validation.lockfiles[importerId] = lockfile ?? null
  }
}

/**
 * Settings whose unset form means the same as a concrete value.
 *
 * The workspace state only records settings that were configured, so a
 * setting left at its default has no key in the state file. The resolved
 * config, on the other hand, may carry the value the default resolves to:
 * `@pnpm/config.reader` writes `enableGlobalVirtualStore: false` when `ci`
 * is set, and reading `allowBuilds` yields `{}`. Normalizing unset values
 * ensures an unrecorded setting matches its resolved default.
 */
const SETTING_UNSET_EQUIVALENTS: Partial<Record<keyof WorkspaceStateSettings, unknown>> = {
  allowBuilds: {},
  enableGlobalVirtualStore: false,
}

export function normalizeUnsetSetting (settingName: keyof WorkspaceStateSettings, value: unknown): unknown {
  return value ?? SETTING_UNSET_EQUIVALENTS[settingName]
}

function settingsMatch (left: WorkspaceStateSettings, right: WorkspaceStateSettings): boolean {
  return WORKSPACE_STATE_SETTING_KEYS.every((key) =>
    equals(normalizeUnsetSetting(key, left[key]), normalizeUnsetSetting(key, right[key]))
  )
}

/**
 * Whether the projects `previous` vouched for were proven against the
 * workspace-wide inputs `state` and `files` record now. A filtered install that
 * changed any of them changed what an unselected project's tree should be too.
 */
function sharedInputsMatch (
  previous: WorkspaceState,
  state: WorkspaceState,
  sharedInputs: string[],
  files: Record<string, FileSignature>
): boolean {
  if (previous.validation == null) return false
  return settingsMatch(previous.settings, state.settings) &&
    equals(previous.pnpmfiles, state.pnpmfiles) &&
    equals(previous.configDependencies ?? {}, state.configDependencies ?? {}) &&
    sharedInputs.every((input) => previous.validation!.files?.[input]?.digest === files[input]?.digest)
}

/**
 * The filesystem clock's current time, from the mtime of a file the
 * filesystem stamps itself in `dir`, so it shares a clock with every file a
 * signature records. `undefined` when the probe cannot be created.
 */
function filesystemNowNs (dir: string): number | undefined {
  const probe = path.join(dir, `.pnpm-validation-probe-${process.pid}-${crypto.randomBytes(4).toString('hex')}`)
  try {
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(probe, '')
    return Number(fs.statSync(probe, { bigint: true }).mtimeNs)
  } catch {
    return undefined
  } finally {
    try {
      fs.rmSync(probe, { force: true })
    } catch {
      // A probe left behind, as a scanner holding it open on Windows leaves
      // it, does not make the clock it read wrong.
    }
  }
}
