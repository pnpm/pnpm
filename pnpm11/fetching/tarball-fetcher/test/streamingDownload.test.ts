import fs from 'node:fs/promises'

import { expect, jest, test } from '@jest/globals'
import type { FetchFromRegistry } from '@pnpm/fetching.types'
import type { Cafs } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'

const addFilesFromTarball = jest.fn<(opts: { buffer?: Buffer, tarballFile?: string }) => Promise<unknown>>()
jest.unstable_mockModule('@pnpm/worker', () => ({ addFilesFromTarball, finishWorkers: async () => {} }))
const { createDownloader } = await import('../src/remoteTarballFetcher.js')

const chunk = Buffer.alloc(1024 * 1024, 0x61)
const byteLength = 65 * chunk.length

async function * body (): AsyncGenerator<Buffer> {
  for (let index = 0; index < 65; index++) yield chunk
}

test.each([null, String(byteLength)])('large download spills to disk (content-length: %s)', async (contentLength) => {
  let filename: string | undefined
  addFilesFromTarball.mockImplementation(async (opts) => {
    filename = opts.tarballFile
    expect(opts.buffer).toBeUndefined()
    expect((await fs.stat(filename!)).size).toBe(byteLength)
    const handle = await fs.open(filename!, 'r')
    try {
      const last = Buffer.alloc(1)
      await handle.read(last, 0, 1, byteLength - 1)
      expect(last[0]).toBe(0x61)
    } finally {
      await handle.close()
    }
    return { local: false }
  })
  const headers = new Headers(contentLength == null ? {} : { 'content-length': contentLength })
  const fetch = (async () => ({ status: 200, headers, body: body() })) as unknown as FetchFromRegistry
  const download = createDownloader(fetch, { retry: { retries: 0 } })
  await download('https://example.com/package.tgz', {
    cafs: { storeDir: '/unused' } as Cafs,
    getAuthHeaderByURI: () => undefined,
    storeIndex: {} as StoreIndex,
    filesIndexFile: 'unused',
  })
  await expect(fs.stat(filename!)).rejects.toMatchObject({ code: 'ENOENT' })
})

test('spilled download is removed when extraction fails', async () => {
  let filename: string | undefined
  addFilesFromTarball.mockImplementation(async (opts) => {
    filename = opts.tarballFile
    throw new Error('extract failure')
  })
  const fetch = (async () => ({ status: 200, headers: new Headers(), body: body() })) as unknown as FetchFromRegistry
  const download = createDownloader(fetch, { retry: { retries: 0 } })
  await expect(download('https://example.com/package.tgz', {
    cafs: { storeDir: '/unused' } as Cafs,
    getAuthHeaderByURI: () => undefined,
    storeIndex: {} as StoreIndex,
    filesIndexFile: 'unused',
  })).rejects.toThrow('extract failure')
  await expect(fs.stat(filename!)).rejects.toMatchObject({ code: 'ENOENT' })
})

test('an enormous declared length does not reserve response-sized memory', async () => {
  const fetch = (async () => ({ status: 200, headers: new Headers({ 'content-length': '1000000000000' }), body: [chunk] })) as unknown as FetchFromRegistry
  const download = createDownloader(fetch, { retry: { retries: 0 } })
  await expect(download('https://example.com/package.tgz', {
    cafs: { storeDir: '/unused' } as Cafs,
    getAuthHeaderByURI: () => undefined,
    storeIndex: {} as StoreIndex,
    filesIndexFile: 'unused',
  })).rejects.toMatchObject({ code: 'ERR_PNPM_BAD_TARBALL_SIZE' })
})
