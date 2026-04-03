import fs from 'node:fs'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

export async function retryLoadJsonFile<T> (filePath: string): Promise<T> {
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    try {
      const data = await fs.promises.readFile(filePath, 'utf8')
      return parseNdjsonMeta(data) as T
    } catch (err: any) { // eslint-disable-line
      if (retry > 2) throw err
      retry++
    }
  }
  /* eslint-enable no-await-in-loop */
}

/**
 * Parses an NDJSON cache file: line 1 = headers, line 2 = metadata.
 * The headers (cachedAt, etag) are merged into the metadata object.
 */
export function parseNdjsonMeta<T> (data: string): T {
  const newlineIdx = data.indexOf('\n')
  if (newlineIdx === -1) return JSON.parse(data) as T
  const headers = JSON.parse(data.slice(0, newlineIdx))
  const meta = JSON.parse(data.slice(newlineIdx + 1))
  return { ...meta, ...headers } as T
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
