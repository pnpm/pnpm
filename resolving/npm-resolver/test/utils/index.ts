import { MetadataCache, type MetadataType } from '@pnpm/cache.metadata'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

export function registryHost (registry?: string): string {
  return new URL(registry ?? 'https://registry.npmjs.org/').host
}

export async function retryLoadFromCache<T> (cacheDir: string, name: string, type: MetadataType, registry?: string): Promise<T> {
  const dbName = `${registryHost(registry)}/${name}`
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    const db = new MetadataCache(cacheDir)
    try {
      const row = db.get(dbName, type)
      if (row) {
        return JSON.parse(row.data) as T
      }
      if (retry > 2) throw new Error(`No cache entry found for ${dbName} (${type})`)
      retry++
    } finally {
      db.close()
    }
  }
  /* eslint-enable no-await-in-loop */
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
