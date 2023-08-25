import path from 'path'
import os from 'os'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'
import { type DeferredManifestPromise } from '@pnpm/cafs-types'
import { PnpmError } from '@pnpm/error'
import { type AddDirToStoreMessage } from './types'

export { type WorkerPool }

const workerPool = createTarballWorkerPool()

export { workerPool }

export function createTarballWorkerPool () {
  const maxWorkers = Math.max(2, os.cpus().length - Math.abs(process.env.PNPM_WORKERS ? parseInt(process.env.PNPM_WORKERS) : 0)) - 1
  const workerPool = new WorkerPool({
    id: 'tarball',
    maxWorkers,
    workerScriptPath: path.join(__dirname, 'tarballWorker.js'),
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
