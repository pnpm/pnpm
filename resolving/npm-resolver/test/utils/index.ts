import { MetadataCache } from '@pnpm/cache.metadata'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

export function registryHost (registry?: string): string {
  return new URL(registry ?? 'https://registry.npmjs.org/').host
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function retryLoadFromCache (cacheDir: string, name: string, _type?: string, registry?: string): Promise<any> {
  const dbName = `${registryHost(registry)}/${name}`
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    const db = new MetadataCache(cacheDir)
    try {
      const row = db.get(dbName)
      if (row) {
        const data = typeof row.data === 'string' ? row.data : Buffer.from(row.data).toString()
        return JSON.parse(data)
      }
      if (retry > 2) throw new Error(`No cache entry found for ${dbName}`)
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
