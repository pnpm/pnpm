import path from 'path'
import os from 'os'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'

export { type WorkerPool }

export function createTarballWorkerPool () {
  const workerPool = new WorkerPool({
    id: 'tarball',
    maxWorkers: os.cpus().length - 1,
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
