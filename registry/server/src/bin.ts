import { createRegistryServer } from './createRegistryServer.js'

const port = parseInt(process.env['PORT'] ?? '4873', 10)
const storeDir = process.env['PNPM_REGISTRY_STORE_DIR'] ?? './store'
const cacheDir = process.env['PNPM_REGISTRY_CACHE_DIR'] ?? './cache'
const upstream = process.env['PNPM_REGISTRY_UPSTREAM'] ?? 'https://registry.npmjs.org/'

async function main (): Promise<void> {
  const server = await createRegistryServer({
    storeDir,
    cacheDir,
    registries: { default: upstream },
    port,
  })

  server.listen(port, () => {
    console.log(`pnpm-registry server listening on http://localhost:${port}`)
    console.log(`  store: ${storeDir}`)
    console.log(`  cache: ${cacheDir}`)
    console.log(`  upstream: ${upstream}`)
  })
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
