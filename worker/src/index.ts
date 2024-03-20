// cspell:ignore checkin
import '@total-typescript/ts-reset'

import os from 'node:os'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'
import type { AddDirToStoreMessage, DependencyManifest, HardLinkDirMessage, LinkPkgMessage, SymlinkAllModulesMessage, TarballExtractMessage } from '@pnpm/types'

let workerPool: WorkerPool | undefined

export async function restartWorkerPool(): Promise<void> {
  await finishWorkers()

  workerPool = createTarballWorkerPool()
}

export async function finishWorkers(): Promise<void> {
  // @ts-expect-error
  await global.finishWorkers?.()
}

function createTarballWorkerPool(): WorkerPool {
  const maxWorkers =
    Math.max(
      2,
      (os.availableParallelism?.() ?? os.cpus().length) -
        Math.abs(
          process.env.PNPM_WORKERS ? parseInt(process.env.PNPM_WORKERS) : 0
        )
    ) - 1

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

export async function addFilesFromDir(
  opts: Pick<
    AddDirToStoreMessage,
    | 'cafsDir'
    | 'dir'
    | 'filesIndexFile'
    | 'sideEffectsCacheKey'
    | 'readManifest'
    | 'pkg'
  >
): Promise<{
    filesIndex: Record<string, string>;
    manifest: DependencyManifest;
  }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  return new Promise<{
    filesIndex: Record<string, string>
    manifest: DependencyManifest
  }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }): void => {
      workerPool?.checkinWorker(localWorker)

      if (status === 'error') {
        reject(
          new PnpmError(
            error.code ?? 'GIT_FETCH_FAILED',
            error.message as string
          )
        )
        return
      }

      resolve(value)
    })

    localWorker.postMessage({
      type: 'add-dir',
      cafsDir: opts.cafsDir,
      dir: opts.dir,
      filesIndexFile: opts.filesIndexFile,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
      readManifest: opts.readManifest,
      pkg: opts.pkg,
    })
  })
}

export class TarballIntegrityError extends PnpmError {
  public readonly found: string
  public readonly expected: string
  public readonly algorithm: string
  public readonly sri: string
  public readonly url: string

  constructor(opts: {
    attempts?: number
    found: string
    expected: string
    algorithm: string
    sri: string
    url: string
  }) {
    super(
      'TARBALL_INTEGRITY',
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

export async function addFilesFromTarball(
  opts: Pick<
    TarballExtractMessage,
    | 'buffer'
    | 'cafsDir'
    | 'filesIndexFile'
    | 'integrity'
    | 'readManifest'
    | 'pkg'
  > & {
    url: string
  }
): Promise<{
    filesIndex: Record<string, string>;
    manifest: DependencyManifest;
  }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  return new Promise<{
    filesIndex: Record<string, string>
    manifest: DependencyManifest
  }>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }) => {
      workerPool?.checkinWorker(localWorker)

      if (status === 'error') {
        if (error.type === 'integrity_validation_failed') {
          reject(
            new TarballIntegrityError({
              ...error,
              url: opts.url,
            })
          )
          return
        }

        reject(
          new PnpmError(
            error.code ?? 'TARBALL_EXTRACT',
            `Failed to add tarball from "${opts.url}" to store: ${error.message as string}`
          )
        )

        return
      }

      resolve(value)
    })

    localWorker.postMessage({
      type: 'extract',
      buffer: opts.buffer,
      cafsDir: opts.cafsDir,
      integrity: opts.integrity,
      filesIndexFile: opts.filesIndexFile,
      readManifest: opts.readManifest,
      pkg: opts.pkg,
    })
  })
}

export async function readPkgFromCafs(
  cafsDir: string,
  verifyStoreIntegrity: boolean,
  filesIndexFile: string,
  readManifest?: boolean | undefined
): Promise<{
    verified: boolean
    pkgFilesIndex: PackageFilesIndex
    manifest?: DependencyManifest | undefined
  }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  return new Promise<{ verified: boolean; pkgFilesIndex: PackageFilesIndex }>(
    (resolve, reject) => {
      localWorker.once('message', ({ status, error, value }) => {
        workerPool?.checkinWorker(localWorker)

        if (status === 'error') {
          reject(
            new PnpmError(
              error.code ?? 'READ_FROM_STORE',
              error.message as string
            )
          )

          return
        }

        resolve(value)
      })

      localWorker.postMessage({
        type: 'readPkgFromCafs',
        cafsDir,
        filesIndexFile,
        readManifest,
        verifyStoreIntegrity,
      })
    }
  )
}

export async function importPackage(
  opts: Omit<LinkPkgMessage, 'type'>
): Promise<{ isBuilt: boolean; importMethod: string | undefined }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  return new Promise<{ isBuilt: boolean; importMethod: string | undefined }>(
    (resolve, reject) => {
      localWorker.once('message', ({ status, error, value }) => {
        workerPool?.checkinWorker(localWorker)

        if (status === 'error') {
          reject(
            new PnpmError(
              error.code ?? 'LINKING_FAILED',
              error.message as string
            )
          )

          return
        }

        resolve(value)
      })

      localWorker.postMessage({
        type: 'link',
        ...opts,
      })
    }
  )
}

export async function symlinkAllModules(
  opts: Omit<SymlinkAllModulesMessage, 'type'>
): Promise<{ isBuilt: boolean; importMethod: string | undefined }> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  return new Promise<{ isBuilt: boolean; importMethod: string | undefined }>(
    (resolve, reject) => {
      localWorker.once('message', (err) => {
        workerPool?.checkinWorker(localWorker)

        if (err.status === 'error') {
          reject(
            new PnpmError(
              err.error.code ?? 'SYMLINK_FAILED',
              err.error.message as string
            )
          )

          return
        }

        resolve(err.value)
      })

      localWorker.postMessage({
        type: 'symlinkAllModules',
        ...opts,
      } satisfies SymlinkAllModulesMessage)
    }
  )
}

export async function hardLinkDir(
  src: string,
  destDirs: string[]
): Promise<void> {
  if (!workerPool) {
    workerPool = createTarballWorkerPool()
  }

  const localWorker = await workerPool.checkoutWorkerAsync(true)

  await new Promise<void>((resolve, reject) => {
    localWorker.once('message', ({ status, error }) => {
      workerPool?.checkinWorker(localWorker)

      if (status === 'error') {
        reject(
          new PnpmError(
            error.code ?? 'HARDLINK_FAILED',
            error.message as string
          )
        )

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
