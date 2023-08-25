import path from 'path'
import os from 'os'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'

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
