// cspell:ignore checkin
import { execSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type { FilesMap, PackageFilesResponse } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import type { BundledManifest } from '@pnpm/types'
import { WorkerPool } from '@rushstack/worker-pool'
import isWindows from 'is-windows'
import pLimit from 'p-limit'

import type {
  AddDirToStoreMessage,
  FetchAndWriteCafsMessage,
  HardLinkDirMessage,
  LinkPkgMessage,
  SymlinkAllModulesMessage,
  TarballExtractMessage,
} from './types.js'

let workerPool: WorkerPool | undefined

export async function restartWorkerPool (): Promise<void> {
  await finishWorkers()
  workerPool = createTarballWorkerPool()
}

export async function finishWorkers (): Promise<void> {
  // @ts-expect-error
  const finish = global.finishWorkers
  // @ts-expect-error
  global.finishWorkers = undefined
  await finish?.()
  workerPool = undefined
}

function createTarballWorkerPool (): WorkerPool {
  const maxWorkers = calcMaxWorkers()
  const workerPool = new WorkerPool({
    id: 'pnpm',
    maxWorkers,
    workerScriptPath: path.join(import.meta.dirname, 'worker.js'),
  })
  // @ts-expect-error
  if (global.finishWorkers) {
    // @ts-expect-error
    const previous = global.finishWorkers
    // @ts-expect-error
    global.finishWorkers = async () => {
      await previous()
      await workerPool.finishAsync()
    }
  } else {
    // @ts-expect-error
    global.finishWorkers = () => workerPool.finishAsync()
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
  integrity?: string
}

type AddFilesFromDirOptions = Pick<AddDirToStoreMessage, 'storeDir' | 'dir' | 'filesIndexFile' | 'sideEffectsCacheKey' | 'readManifest' | 'pkg' | 'files' | 'appendManifest' | 'includeNodeModules'> & {
  storeIndex: StoreIndex
}

export async function addFilesFromDir (opts: AddFilesFromDirOptions): Promise<AddFilesResult> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<AddFilesResult>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value, indexWrites }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'GIT_FETCH_FAILED', error.message as string))
        return
      }
      if (indexWrites) {
        // Write immediately so that subsequent worker reads (e.g. side effects)
        // see the committed data without waiting for nextTick.
        opts.storeIndex.setRawMany(indexWrites)
      }
      resolve(value)
    })
    localWorker.postMessage({
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
    })
  })
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
      `Got unexpected checksum for "${opts.url}". Wanted "${opts.expected}". Got "${opts.found}".`,
      {
        attempts: opts.attempts,
        hint: `This error may happen when a package is republished to the registry with the same version.
In this case, the metadata in the local pnpm cache will contain the old integrity checksum.

If you think that this is the case, then run "pnpm store prune" and rerun the command that failed.
"pnpm store prune" will remove your local metadata cache.`,
      }
    )
    this.found = opts.found
    this.expected = opts.expected
    this.algorithm = opts.algorithm
    this.sri = opts.sri
    this.url = opts.url
  }
}

type AddFilesFromTarballOptions = Pick<TarballExtractMessage, 'buffer' | 'storeDir' | 'filesIndexFile' | 'integrity' | 'readManifest' | 'pkg' | 'appendManifest'> & {
  storeIndex: StoreIndex
  url: string
}

export async function addFilesFromTarball (opts: AddFilesFromTarballOptions): Promise<AddFilesResult> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<AddFilesResult>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value, indexWrites }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        if (error.type === 'integrity_validation_failed') {
          reject(new TarballIntegrityError({
            ...error,
            url: opts.url,
          }))
          return
        }
        reject(new PnpmError(error.code ?? 'TARBALL_EXTRACT', `Failed to add tarball from "${opts.url}" to store: ${error.message as string}`))
        return
      }
      if (indexWrites) {
        opts.storeIndex.queueWrites(indexWrites)
      }
      resolve(value)
    })
    localWorker.postMessage({
      type: 'extract',
      buffer: opts.buffer,
      storeDir: opts.storeDir,
      integrity: opts.integrity,
      filesIndexFile: opts.filesIndexFile,
      readManifest: opts.readManifest,
      pkg: opts.pkg,
      appendManifest: opts.appendManifest,
    })
  })
}


export async function fetchAndWriteCafsFiles (opts: {
  registryUrl: string
  storeDir: string
  digests: FetchAndWriteCafsMessage['digests']
}): Promise<number> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<number>((resolve, reject) => {
    localWorker.once('message', ({ status, error, filesWritten }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError('CAFS_FETCH_WRITE', error.message))
        return
      }
      resolve(filesWritten)
    })
    localWorker.postMessage({
      type: 'fetch-and-write-cafs',
      registryUrl: opts.registryUrl,
      storeDir: opts.storeDir,
      digests: opts.digests,
    } satisfies FetchAndWriteCafsMessage)
  })
}

export interface ReadPkgFromCafsContext {
  storeDir: string
  verifyStoreIntegrity: boolean
  strictStorePkgContentCheck?: boolean
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
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise((resolve, reject) => {
    localWorker.once('message', ({ status, error, value, warnings }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'READ_FROM_STORE', error.message as string, { hint: error.hint }))
        return
      }
      if (warnings) {
        for (const warning of warnings) {
          globalWarn(warning)
        }
      }
      resolve(value)
    })
    localWorker.postMessage({
      type: 'readPkgFromCafs',
      filesIndexFile,
      ...ctx,
      ...opts,
    })
  })
}

// The workers are doing lots of file system operations
// so, running them in parallel helps only to a point.
// With local experimenting it was discovered that running 4 workers gives the best results.
// Adding more workers actually makes installation slower.
let limitImportingPackage = pLimit(4)

/**
 * Temporarily change import concurrency. Called by the pnpm agent code path
 * where there's no concurrent fetching competing for workers. Returns a
 * disposer that restores the previous limiter — callers must invoke it (in a
 * finally block) to avoid leaking the mutation to other installs in the same
 * process (e.g. test suites).
 *
 * If two installs overlap, the disposer for the outer install would otherwise
 * clobber the inner one's still-active limiter. Each disposer captures the
 * limiter it installed and only restores when it's still the active one,
 * leaving any newer override in place.
 */
export function setImportConcurrency (concurrency: number): () => void {
  if (!Number.isInteger(concurrency) || concurrency < 1) {
    throw new Error(`setImportConcurrency: expected a positive integer, got ${concurrency}`)
  }
  const previous = limitImportingPackage
  const installed = pLimit(concurrency)
  limitImportingPackage = installed
  return () => {
    if (limitImportingPackage === installed) {
      limitImportingPackage = previous
    }
  }
}

export async function importPackage (
  opts: Omit<LinkPkgMessage, 'type'>
): Promise<{ isBuilt: boolean, importMethod: string | undefined }> {
  return limitImportingPackage(async () => {
    if (!workerPool) {
      workerPool = createTarballWorkerPool()
    }
    const localWorker = await workerPool.checkoutWorkerAsync(true)
    return new Promise<{ isBuilt: boolean, importMethod: string | undefined }>((resolve, reject) => {
      localWorker.once('message', ({ status, error, value }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
        workerPool!.checkinWorker(localWorker)
        if (status === 'error') {
          reject(new PnpmError(error.code ?? 'LINKING_FAILED', `[importPackage ${opts.targetDir}] ${error.message as string}`))
          return
        }
        resolve(value)
      })
      localWorker.postMessage({
        type: 'link',
        ...opts,
      })
    })
  })
}

export async function symlinkAllModules (
  opts: Omit<SymlinkAllModulesMessage, 'type'>
): Promise<{ isBuilt: boolean, importMethod: string | undefined }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<{ isBuilt: boolean, importMethod: string | undefined }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        const hint = opts.deps?.[0]?.modules != null ? createErrorHint(error, opts.deps[0].modules) : undefined
        reject(new PnpmError(error.code ?? 'SYMLINK_FAILED', `[symlinkAllModules] ${error.message as string}`, { hint }))
        return
      }
      resolve(value)
    })
    localWorker.postMessage({
      type: 'symlinkAllModules',
      ...opts,
    } as SymlinkAllModulesMessage)
  })
}

function createErrorHint (err: Error, checkedDir: string): string | undefined {
  if ('code' in err && err.code === 'EISDIR' && isWindows()) {
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
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  await new Promise<void>((resolve, reject) => {
    localWorker.once('message', ({ status, error }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'HARDLINK_FAILED', error.message as string))
        return
      }
      resolve()
    })
    localWorker.postMessage({
      type: 'hardLinkDir',
      src,
      destDirs,
    } as HardLinkDirMessage)
  })
}

export async function initStoreDir (storeDir: string): Promise<void> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<void>((resolve, reject) => {
    localWorker.once('message', ({ status, error }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'INIT_CAFS_FAILED', error.message as string))
        return
      }
      resolve()
    })
    localWorker.postMessage({
      type: 'init-store',
      storeDir,
    })
  })
}
