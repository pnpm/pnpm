import fs from 'node:fs'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

/**
 * Read a metadata mirror that a fire-and-forget write may not have produced
 * yet. `isReady` separates a stale read from the one the caller waits for: a
 * mirror that already exists parses fine long before the write under test
 * replaces it.
 *
 * Resolves with the first parsed document `isReady` accepts, polling up to
 * four times 500ms apart. When none does, it rejects with the last read or
 * parse error, or with one naming the file if every attempt was merely not
 * ready yet.
 */
export async function retryLoadJsonFile<Parsed> (filePath: string, isReady?: (data: Parsed) => boolean): Promise<Parsed> {
  let lastError: unknown
  /* eslint-disable no-await-in-loop -- each attempt waits for the previous one to fail */
  for (let attempt = 0; attempt < 4; attempt++) {
    await delay(500)
    try {
      const data = parseNdjsonMeta<Parsed>(await fs.promises.readFile(filePath, 'utf8'))
      if (isReady == null || isReady(data)) return data
      lastError = new Error(`${filePath} does not hold the awaited metadata yet`)
    } catch (err: unknown) {
      lastError = err
    }
  }
  /* eslint-enable no-await-in-loop */
  throw lastError
}

/**
 * Parses an NDJSON cache file: line 1 = headers, line 2 = metadata.
 * The headers (etag, modified) are merged into the metadata object.
 */
export function parseNdjsonMeta<Parsed> (data: string): Parsed {
  const newlineIdx = data.indexOf('\n')
  if (newlineIdx === -1) return JSON.parse(data) as Parsed
  const headers = JSON.parse(data.slice(0, newlineIdx))
  const meta = JSON.parse(data.slice(newlineIdx + 1))
  return { ...meta, ...headers } as Parsed
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
