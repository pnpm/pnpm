import AdmZip from 'adm-zip'
import { Response } from 'node-fetch'
import path from 'path'
import { Readable } from 'stream'
import { fetchNode, FetchNodeOptions } from '@pnpm/node.fetcher'
import { tempDir } from '@pnpm/prepare'

const fetchMock = jest.fn(async (url: string) => {
  if (url.endsWith('.zip')) {
    // The Windows code path for pnpm's node bootstrapping expects a subdir
    // within the .zip file.
    const pkgName = path.basename(url, '.zip')
    const zip = new AdmZip()
    zip.addFile(`${pkgName}/dummy-file`, Buffer.from('test'))

    return new Response(Readable.from(zip.toBuffer()))
  }

  return new Response(Readable.from(Buffer.alloc(0)))
})

beforeEach(() => {
  fetchMock.mockClear()
})

test('install Node using a custom node mirror', async () => {
  tempDir()

  const nodeMirrorBaseUrl = 'https://pnpm-node-mirror-test.localhost/download/release/'
  const opts: FetchNodeOptions = {
    nodeMirrorBaseUrl,
    cafsDir: path.resolve('files'),
  }

  await fetchNode(fetchMock, '16.4.0', path.resolve('node'), opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch(nodeMirrorBaseUrl)
  }
})

test('install Node using the default node mirror', async () => {
  tempDir()

  const opts: FetchNodeOptions = {
    cafsDir: path.resolve('files'),
  }

  await fetchNode(fetchMock, '16.4.0', path.resolve('node'), opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch('https://nodejs.org/download/release/')
  }
})
