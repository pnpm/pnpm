// cspell:ignore checkin
import path from 'path'
import os from 'os'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'
import { PnpmError } from '@pnpm/error'
import { execSync } from 'child_process'
import isWindows from 'is-windows'
import { type PackageFilesIndex } from '@pnpm/store.cafs'
import { type DependencyManifest } from '@pnpm/types'
import { quote as shellQuote } from 'shell-quote'
import {
  type TarballExtractMessage,
  type AddDirToStoreMessage,
  type LinkPkgMessage,
  type SymlinkAllModulesMessage,
  type HardLinkDirMessage,
} from './types'

let workerPool: WorkerPool | undefined

export async function restartWorkerPool (): Promise<void> {
  await finishWorkers()
  workerPool = createTarballWorkerPool()
}

export async function finishWorkers (): Promise<void> {
  // @ts-expect-error
  await global.finishWorkers?.()
}

function createTarballWorkerPool (): WorkerPool {
  const maxWorkers = Math.max(2, (os.availableParallelism?.() ?? os.cpus().length) - Math.abs(process.env.PNPM_WORKERS ? parseInt(process.env.PNPM_WORKERS) : 0)) - 1
  const workerPool = new WorkerPool({
    id: 'pnpm',
    maxWorkers,
    workerScriptPath: path.join(__dirname, 'worker.js'),
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

interface AddFilesResult {
  filesIndex: Record<string, string>
  manifest: DependencyManifest
  requiresBuild: boolean
}

type AddFilesFromDirOptions = Pick<AddDirToStoreMessage, 'storeDir' | 'dir' | 'filesIndexFile' | 'sideEffectsCacheKey' | 'readManifest' | 'pkg' | 'files'>

export async function addFilesFromDir (opts: AddFilesFromDirOptions): Promise<AddFilesResult> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<{ filesIndex: Record<string, string>, manifest: DependencyManifest, requiresBuild: boolean }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'GIT_FETCH_FAILED', error.message as string))
        return
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
      files: opts.files,
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

type AddFilesFromTarballOptions = Pick<TarballExtractMessage, 'buffer' | 'storeDir' | 'filesIndexFile' | 'integrity' | 'readManifest' | 'pkg'> & {
  url: string
}

export async function addFilesFromTarball (opts: AddFilesFromTarballOptions): Promise<AddFilesResult> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<{ filesIndex: Record<string, string>, manifest: DependencyManifest, requiresBuild: boolean }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }) => {
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
    })
  })
}

export async function readPkgFromCafs (
  storeDir: string,
  verifyStoreIntegrity: boolean,
  filesIndexFile: string,
  readManifest?: boolean
): Promise<{ verified: boolean, pkgFilesIndex: PackageFilesIndex, manifest?: DependencyManifest, requiresBuild: boolean }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<{ verified: boolean, pkgFilesIndex: PackageFilesIndex, requiresBuild: boolean }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }) => {
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'READ_FROM_STORE', error.message as string))
        return
      }
      resolve(value)
    })
    localWorker.postMessage({
      type: 'readPkgFromCafs',
      storeDir,
      filesIndexFile,
      readManifest,
      verifyStoreIntegrity,
    })
  })
}

export async function importPackage (
  opts: Omit<LinkPkgMessage, 'type'>
): Promise<{ isBuilt: boolean, importMethod: string | undefined }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<{ isBuilt: boolean, importMethod: string | undefined }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
      workerPool!.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError(error.code ?? 'LINKING_FAILED', error.message as string))
        return
      }
      resolve(value)
    })
    localWorker.postMessage({
      type: 'link',
      ...opts,
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
        reject(new PnpmError(error.code ?? 'SYMLINK_FAILED', error.message as string, { hint }))
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
  try {
    const output = execSync(`wmic logicaldisk where ${shellQuote([`DeviceID='${drive}'`])} get FileSystem`).toString()
    const lines = output.trim().split('\n')
    const name = lines.length > 1 ? lines[1].trim() : ''
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
