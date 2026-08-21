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
export async function retryLoadJsonFile<T> (filePath: string, isReady?: (data: T) => boolean): Promise<T> {
  let lastError: unknown
  /* eslint-disable no-await-in-loop */
  for (let attempt = 0; attempt < 4; attempt++) {
    await delay(500)
    try {
      const data = parseNdjsonMeta<T>(await fs.promises.readFile(filePath, 'utf8'))
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
 * Parses a mirror cache file in either layout: the indexed
 * `pnpm-meta-v1` form (headers record, index record, version fragments)
 * or two-line NDJSON (line 1 = headers, line 2 = metadata). The headers
 * (etag, modified) are merged into the metadata object, and versions are
 * materialized eagerly so tests can assert on plain objects.
 */
export function parseNdjsonMeta<T> (data: string): T {
  const buf = Buffer.from(data, 'utf8')
  const newlineIdx = buf.indexOf(10)
  if (newlineIdx === -1) return JSON.parse(data) as T
  const firstLine = buf.toString('utf8', 0, newlineIdx)
  const formatMatch = /^pnpm-meta-v1 (\d+) (\d+)$/.exec(firstLine)
  if (formatMatch == null) {
    const headers = JSON.parse(firstLine)
    const meta = JSON.parse(buf.toString('utf8', newlineIdx + 1))
    return { ...meta, ...headers } as T
  }
  const headersStart = newlineIdx + 1
  const indexStart = headersStart + Number.parseInt(formatMatch[1], 10)
  const fragmentBase = indexStart + Number.parseInt(formatMatch[2], 10)
  const headers = JSON.parse(buf.toString('utf8', headersStart, indexStart))
  const index = JSON.parse(buf.toString('utf8', indexStart, fragmentBase))
  const versions: Record<string, unknown> = {}
  for (const [version, offset, length] of index.versions as Array<[string, number, number]>) {
    versions[version] = JSON.parse(buf.toString('utf8', fragmentBase + offset, fragmentBase + offset + length))
  }
  const meta: Record<string, unknown> = {
    name: index.name,
    'dist-tags': index.distTags ?? {},
    versions,
  }
  if (index.time != null) meta.time = index.time
  return { ...meta, ...headers } as T
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
