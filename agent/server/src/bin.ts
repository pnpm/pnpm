import cluster from 'node:cluster'
import os from 'node:os'

import { createRegistryServer } from './createRegistryServer.js'

const port = parseInt(process.env['PORT'] ?? '4873', 10)
const storeDir = process.env['PNPM_AGENT_STORE_DIR'] ?? './store'
const cacheDir = process.env['PNPM_AGENT_CACHE_DIR'] ?? './cache'
const upstream = process.env['PNPM_AGENT_UPSTREAM'] ?? 'https://registry.npmjs.org/'
const numWorkers = parseInt(process.env['PNPM_AGENT_WORKERS'] ?? String(Math.max(2, os.availableParallelism() - 1)), 10)

if (cluster.isPrimary) {
  console.log(`pnpm agent server starting on http://localhost:${port}`)
  console.log(`  store: ${storeDir}`)
  console.log(`  cache: ${cacheDir}`)
  console.log(`  upstream: ${upstream}`)
  console.log(`  workers: ${numWorkers}`)

  for (let i = 0; i < numWorkers; i++) {
    cluster.fork()
  }

  cluster.on('exit', (worker, code) => {
    if (code !== 0) {
      console.log(`Worker ${worker.process.pid} died (code ${code}), restarting...`)
      cluster.fork()
    }
  })
} else {
  startWorker()
}

async function startWorker (): Promise<void> {
  const server = await createRegistryServer({
    storeDir,
    cacheDir,
    registries: { default: upstream },
  })

  server.listen(port, () => {
    console.log(`  worker ${process.pid} listening`)
  })
}
