import path from 'path'
import os from 'os'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'
import { type DeferredManifestPromise } from '@pnpm/cafs-types'
import { PnpmError } from '@pnpm/error'
import { type TarballExtractMessage, type AddDirToStoreMessage } from './types'

export { type WorkerPool }

const workerPool = createTarballWorkerPool()

export { workerPool }

function createTarballWorkerPool () {
  const maxWorkers = Math.max(2, os.cpus().length - Math.abs(process.env.PNPM_WORKERS ? parseInt(process.env.PNPM_WORKERS) : 0)) - 1
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

export async function addFilesFromDir (
  opts: Pick<AddDirToStoreMessage, 'cafsDir' | 'dir' | 'filesIndexFile' | 'sideEffectsCacheKey'> & {
    manifest?: DeferredManifestPromise
  }
) {
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<Record<string, string>>((resolve, reject) => {
    // eslint-disalbe-next-line
    localWorker.once('message', ({ status, error, value }) => {
      workerPool.checkinWorker(localWorker)
      if (status === 'error') {
        reject(new PnpmError('GIT_FETCH_FAILED', error as string))
        return
      }
      opts.manifest?.resolve(value.manifest)
      resolve(value.filesIndex)
    })
    localWorker.postMessage({
      type: 'add-dir',
      cafsDir: opts.cafsDir,
      dir: opts.dir,
      filesIndexFile: opts.filesIndexFile,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
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

export async function addFilesFromTarball (
  opts: Pick<TarballExtractMessage, 'buffer' | 'cafsDir' | 'filesIndexFile' | 'integrity'> & {
    url: string
    manifest?: DeferredManifestPromise
  }
) {
  const localWorker = await workerPool.checkoutWorkerAsync(true)
  return new Promise<Record<string, string>>((resolve, reject) => {
    localWorker.once('message', ({ status, error, value }) => {
      workerPool.checkinWorker(localWorker)
      if (status === 'error') {
        if (error.type === 'integrity_validation_failed') {
          reject(new TarballIntegrityError({
            ...error,
            url: opts.url,
          }))
          return
        }
        reject(new PnpmError('TARBALL_EXTRACT', `Failed to unpack the tarball from "${opts.url}": ${error as string}`))
        return
      }
      opts.manifest?.resolve(value.manifest)
      resolve(value.filesIndex)
    })
    localWorker.postMessage({
      type: 'extract',
      buffer: opts.buffer,
      cafsDir: opts.cafsDir,
      integrity: opts.integrity,
      filesIndexFile: opts.filesIndexFile,
    })
  })
}
