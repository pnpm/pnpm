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
 * Parses a mirror cache file in either layout: the indexed
 * `pacquet-meta-v1` form (headers record, index record, version fragments)
 * or two-line NDJSON (line 1 = headers, line 2 = metadata). The headers
 * (etag, modified) are merged into the metadata object, and versions are
 * materialized eagerly so tests can assert on plain objects.
 */
export function parseNdjsonMeta<T> (data: string): T {
  const buf = Buffer.from(data, 'utf8')
  const newlineIdx = buf.indexOf(10)
  if (newlineIdx === -1) return JSON.parse(data) as T
  const firstLine = buf.toString('utf8', 0, newlineIdx)
  const magicMatch = /^pacquet-meta-v1 (\d+) (\d+)$/.exec(firstLine)
  if (magicMatch == null) {
    const headers = JSON.parse(firstLine)
    const meta = JSON.parse(buf.toString('utf8', newlineIdx + 1))
    return { ...meta, ...headers } as T
  }
  const headersStart = newlineIdx + 1
  const indexStart = headersStart + Number.parseInt(magicMatch[1], 10)
  const fragmentBase = indexStart + Number.parseInt(magicMatch[2], 10)
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
