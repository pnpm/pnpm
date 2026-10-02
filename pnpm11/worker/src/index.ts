// cspell:ignore checkin
import { execSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'

import { PnpmError, redactAndSanitize, redactUrlForDisplay } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type { VerifiedFileIntegrity } from '@pnpm/store.cafs'
import type { FilesMap, PackageFilesResponse, SideEffectsDiff } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import type { BundledManifest } from '@pnpm/types'
import { WorkerPool } from '@rushstack/worker-pool'
import isWindows from 'is-windows'
import pLimit from 'p-limit'

import type {
  AddDirToStoreMessage,
  HardLinkDirMessage,
  LinkPkgMessage,
  SymlinkAllModulesMessage,
  TarballExtractMessage,
} from './types.js'

let workerPool: WorkerPool | undefined

const globalWithWorkers = globalThis as typeof globalThis & { finishWorkers?: () => Promise<void> }

/**
 * Store verification runs in the workers, so each one tallies the files
 * it re-hashed and the time that took, and hands its share back with
 * every response (see `takeVerifiedFileIntegrity` in `@pnpm/store.cafs`).
 * This is the sum across all of them for the lifetime of the process.
 *
 * An install reports its own share by snapshotting this when it starts
 * and diffing at the end. That is exact for one install at a time,
 * which is every install except the per-project loop a recursive
 * command with dedicated lockfiles runs at `workspaceConcurrency`:
 * there, overlapping projects can pick up each other's hashing.
 *
 * Separating those would take an `AsyncLocalStorage` scope per install,
 * which is not worth it here: constructing one costs the whole process
 * ~5x on promise-heavy work under Node's pre-`AsyncContextFrame`
 * implementation (44ms -> 245ms over 2M awaits, paid whether or not the
 * scope is entered), and pnpm supports Node 22.13, where that is the
 * implementation. Every install would pay it so that a diagnostic reads
 * correctly in one non-default configuration. The alternative with no
 * global cost is threading the tally through the store controller's
 * per-request options into `readPkgFromCafs`.
 */
const verifiedFileIntegrity: VerifiedFileIntegrity = { files: 0, ms: 0 }

export function verifiedFileIntegritySnapshot (): VerifiedFileIntegrity {
  return { ...verifiedFileIntegrity }
}

export function verifiedFileIntegritySince (baseline: VerifiedFileIntegrity): VerifiedFileIntegrity {
  return {
    files: verifiedFileIntegrity.files - baseline.files,
    ms: verifiedFileIntegrity.ms - baseline.ms,
  }
}

function addVerifiedFileIntegrity (reported: VerifiedFileIntegrity | undefined): void {
  if (reported == null) return
  verifiedFileIntegrity.files += reported.files
  verifiedFileIntegrity.ms += reported.ms
}

export async function restartWorkerPool (): Promise<void> {
  await finishWorkers()
  workerPool = createTarballWorkerPool()
}

export async function finishWorkers (): Promise<void> {
  const finish = globalWithWorkers.finishWorkers
  globalWithWorkers.finishWorkers = undefined
  await finish?.()
}

function createTarballWorkerPool (): WorkerPool {
  const maxWorkers = calcMaxWorkers()
  const workerPool = new WorkerPool({
    id: 'pnpm',
    maxWorkers,
    workerScriptPath: path.join(import.meta.dirname, 'worker.js'),
  })
  const previous = globalWithWorkers.finishWorkers
  if (previous) {
    globalWithWorkers.finishWorkers = async () => {
      await previous()
      await workerPool.finishAsync()
    }
  } else {
    globalWithWorkers.finishWorkers = () => workerPool.finishAsync()
  }
  return workerPool
}

export function calcMaxWorkers (): number {
  if (process.env.PNPM_MAX_WORKERS) {
    return parseInt(process.env.PNPM_MAX_WORKERS)
  }
  if (process.env.PNPM_WORKERS) {
    const idleCPUs = Math.abs(parseInt(process.env.PNPM_WORKERS))
    return Math.max(2, availableParallelism() - idleCPUs) - 1
  }
  return Math.max(1, availableParallelism() - 1)
}

function availableParallelism (): number {
  return os.availableParallelism?.() ?? os.cpus().length
}

interface AddFilesResult {
  filesMap: FilesMap
  manifest?: BundledManifest
  requiresBuild: boolean
  requiresPrepare?: boolean
  integrity?: string
  sideEffects?: SideEffectsDiff
}

type AddFilesFromDirOptions = Pick<AddDirToStoreMessage, 'storeDir' | 'dir' | 'filesIndexFile' | 'sideEffectsCacheKey' | 'readManifest' | 'pkg' | 'files' | 'appendManifest' | 'includeNodeModules' | 'requiresPrepare'> & {
  storeIndex: StoreIndex
}

interface WorkerError {
  code?: string
  message: string
  hint?: string
  type?: string
  algorithm?: string
  expected?: string
  found?: string
  sri?: string
}

/**
 * What a worker posts back. `error` is set only when `status` is `'error'`,
 * and `value` only when it is `'success'`.
 */
interface WorkerResponse<Value> {
  status: 'success' | 'error'
  error: WorkerError
  value: Value
  indexWrites?: Array<{ key: string, buffer: Uint8Array }>
  warnings?: string[]
  verifiedFileIntegrity?: VerifiedFileIntegrity
}

/**
 * Posts `message` to a pooled worker and settles with what `settle` returns
 * for the worker's response. A throw from `settle` rejects the promise
 * rather than escaping the message callback, where it would surface as an
 * uncaughtException and leave the promise pending.
 */
async function runInWorker<Value, Result = Value> (
  message: object,
  settle: (response: WorkerResponse<Value>) => Result
): Promise<Result> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<Result>((resolve, reject) => {
    localWorker.once('message', (response: WorkerResponse<Value>) => {
      workerPool!.checkinWorker(localWorker)
      try {
        resolve(settle(response))
      } catch (err: unknown) {
        reject(err as Error)
      }
    })
    localWorker.postMessage(message)
  })
}

export async function addFilesFromDir (opts: AddFilesFromDirOptions): Promise<AddFilesResult> {
  const message: AddDirToStoreMessage = {
    type: 'add-dir',
    storeDir: opts.storeDir,
    dir: opts.dir,
    filesIndexFile: opts.filesIndexFile,
    sideEffectsCacheKey: opts.sideEffectsCacheKey,
    readManifest: opts.readManifest,
    pkg: opts.pkg,
    appendManifest: opts.appendManifest,
    files: opts.files,
    includeNodeModules: opts.includeNodeModules,
    requiresPrepare: opts.requiresPrepare,
  }
  return runInWorker<AddFilesResult>(message, ({ status, error, value, indexWrites }) => {
    if (status === 'error') {
      throw new PnpmError(error.code ?? 'GIT_FETCH_FAILED', error.message)
    }
    if (indexWrites) {
      // Write immediately so that subsequent worker reads (e.g. side effects)
      // see the committed data without waiting for nextTick.
      // A throw here (e.g. ImmutableStoreIndex refusing the write under
      // frozenStore) rejects the promise.
      opts.storeIndex.setRawMany(indexWrites)
    }
    return value
  })
}

/**
 * A remote tarball URL with its credentials, query, and fragment removed. A
 * local tarball is identified by its absolute file path, which is shown with
 * only its control characters removed.
 */
function displayTarballLocation (location: string): string {
  return path.isAbsolute(location) || path.win32.isAbsolute(location)
    ? redactAndSanitize(location)
    : redactUrlForDisplay(location)
}

export class TarballIntegrityError extends PnpmError {
  public readonly found: string
  public readonly expected: string
  public readonly algorithm: string
  public readonly sri: string
  public readonly url: string

  constructor (opts: {
    attempts?: number
    found: string
    expected: string
    algorithm: string
    sri: string
    url: string
  }) {
    super('TARBALL_INTEGRITY',
      `Got unexpected checksum for "${displayTarballLocation(opts.url)}". Wanted "${opts.expected}". Got "${opts.found}".`,
      {
        attempts: opts.attempts,
        hint: `The downloaded tarball does not match the integrity recorded in the lockfile. pnpm will not silently overwrite the locked integrity — that would defeat the lockfile's protection if a registry or proxy is serving tampered content.

If you trust the new content (legitimate republish, or stale local metadata cache):

  - Run "pnpm store prune" and retry, in case only the metadata cache is out of date.
  - Run "pnpm install --update-checksums" to refresh the locked integrity from the registry.

If you did not expect this package to change, treat it as a potential supply-chain issue and verify the new content before re-running with --update-checksums.`,
      }
    )
    this.found = opts.found
    this.expected = opts.expected
    this.algorithm = opts.algorithm
    this.sri = opts.sri
    this.url = opts.url
  }
}

type AddFilesFromTarballOptions = Pick<TarballExtractMessage, 'buffer' | 'storeDir' | 'filesIndexFile' | 'pkgId' | 'integrity' | 'readManifest' | 'pkg' | 'appendManifest' | 'ignoreFilePattern'> & {
  storeIndex: StoreIndex
  url: string
}

export async function addFilesFromTarball (opts: AddFilesFromTarballOptions): Promise<AddFilesResult> {
  const message: TarballExtractMessage = {
    type: 'extract',
    buffer: opts.buffer,
    storeDir: opts.storeDir,
    integrity: opts.integrity,
    filesIndexFile: opts.filesIndexFile,
    pkgId: opts.pkgId,
    readManifest: opts.readManifest,
    pkg: opts.pkg,
    appendManifest: opts.appendManifest,
    ignoreFilePattern: opts.ignoreFilePattern,
  }
  return runInWorker<AddFilesResult>(message, ({ status, error, value, indexWrites }) => {
    if (status === 'error') {
      throw createTarballExtractError(error, opts.url)
    }
    if (indexWrites) {
      opts.storeIndex.queueWrites(indexWrites)
    }
    return value
  })
}

function createTarballExtractError (error: WorkerError, url: string): PnpmError {
  if (error.type === 'integrity_validation_failed') {
    return new TarballIntegrityError({
      ...error,
      url,
    } as ConstructorParameters<typeof TarballIntegrityError>[0])
  }
  return new PnpmError(error.code ?? 'TARBALL_EXTRACT', `Failed to add tarball from "${url}" to store: ${error.message}`)
}


export interface ReadPkgFromCafsContext {
  storeDir: string
  verifyStoreIntegrity: boolean
  strictStorePkgContentCheck?: boolean
  frozenStore?: boolean
}

export interface ReadPkgFromCafsOptions {
  readManifest?: boolean
  expectedPkg?: { name?: string, version?: string }
}

export interface ReadPkgFromCafsResult {
  verified: boolean
  files: PackageFilesResponse
  bundledManifest?: BundledManifest
}

export async function readPkgFromCafs (
  ctx: ReadPkgFromCafsContext,
  filesIndexFile: string,
  opts?: ReadPkgFromCafsOptions
): Promise<ReadPkgFromCafsResult> {
  const message = {
    type: 'readPkgFromCafs',
    filesIndexFile,
    ...ctx,
    ...opts,
  }
  return runInWorker<ReadPkgFromCafsResult>(message, ({ status, error, value, warnings, verifiedFileIntegrity }) => {
    addVerifiedFileIntegrity(verifiedFileIntegrity)
    if (status === 'error') {
      throw new PnpmError(error.code ?? 'READ_FROM_STORE', error.message, { hint: error.hint })
    }
    for (const warning of warnings ?? []) {
      globalWarn(warning)
    }
    return value
  })
}

interface ImportPackageResult {
  isBuilt: boolean
  importMethod: string | undefined
}

// The workers are doing lots of file system operations
// so, running them in parallel helps only to a point.
// With local experimenting it was discovered that running 4 workers gives the best results.
// Adding more workers actually makes installation slower.
const limitImportingPackage = pLimit(4)

export async function importPackage (
  opts: Omit<LinkPkgMessage, 'type'>
): Promise<ImportPackageResult> {
  return limitImportingPackage(async () => runInWorker<ImportPackageResult>({
    type: 'link',
    ...opts,
  }, ({ status, error, value }) => {
    if (status === 'error') {
      throw new PnpmError(error.code ?? 'LINKING_FAILED', `[importPackage ${opts.targetDir}] ${error.message}`)
    }
    return value
  }))
}

export async function symlinkAllModules (
  opts: Omit<SymlinkAllModulesMessage, 'type'>
): Promise<ImportPackageResult> {
  return runInWorker<ImportPackageResult>({
    type: 'symlinkAllModules',
    ...opts,
  } as SymlinkAllModulesMessage, ({ status, error, value }) => {
    if (status === 'error') {
      const hint = opts.deps?.[0]?.modules != null ? createErrorHint(error, opts.deps[0].modules) : undefined
      throw new PnpmError(error.code ?? 'SYMLINK_FAILED', `[symlinkAllModules] ${error.message}`, { hint })
    }
    return value
  })
}

function createErrorHint (err: WorkerError, checkedDir: string): string | undefined {
  if (err.code === 'EISDIR' && isWindows()) {
    const checkedDrive = `${checkedDir.split(':')[0]}:`
    if (isDriveExFat(checkedDrive)) {
      return `The "${checkedDrive}" drive is exFAT, which does not support symlinks. This will cause installation to fail. You can set the node-linker to "hoisted" to avoid this issue.`
    }
  }
  return undefined
}

// In Windows system exFAT drive, symlink will result in error.
function isDriveExFat (drive: string): boolean {
  if (!/^[a-z]:$/i.test(drive)) {
    throw new Error(`${drive} is not a valid disk on Windows`)
  }
  try {
    // cspell:disable-next-line
    const output = execSync(`powershell -Command "Get-Volume -DriveLetter ${drive.replace(':', '')} | Select-Object -ExpandProperty FileSystem"`).toString()
    const lines = output.trim().split('\n')
    const name = lines[0].trim()
    return name === 'exFAT'
  } catch {
    return false
  }
}

export async function hardLinkDir (src: string, destDirs: string[]): Promise<void> {
  await runInWorker<undefined, void>({
    type: 'hardLinkDir',
    src,
    destDirs,
  } as HardLinkDirMessage, ({ status, error }) => {
    if (status === 'error') {
      throw new PnpmError(error.code ?? 'HARDLINK_FAILED', error.message)
    }
  })
}

export async function initStoreDir (storeDir: string): Promise<void> {
  return runInWorker<undefined, void>({
    type: 'init-store',
    storeDir,
  }, ({ status, error }) => {
    if (status === 'error') {
      throw new PnpmError(error.code ?? 'INIT_CAFS_FAILED', error.message)
    }
  })
}
