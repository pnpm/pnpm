import AdmZip from 'adm-zip'
import { Response } from 'node-fetch'
import path from 'path'
import { Readable } from 'stream'
import type { FetchNodeOptionsToDir as FetchNodeOptions } from '@pnpm/node.fetcher'
import { tempDir } from '@pnpm/prepare'
import { jest } from '@jest/globals'

jest.unstable_mockModule('detect-libc', () => ({
  isNonGlibcLinux: jest.fn(),
}))

const { fetchNode } = await import('@pnpm/node.fetcher')
const { isNonGlibcLinux } = await import('detect-libc')

// A stable fake hex digest used as placeholder sha256 in mock SHASUMS256.txt files.
// Any non-zero value works; the tarball content won't match, so integrity will
// fail — but all URL assertions run before that happens.
const FAKE_SHA256 = '5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef'

const fetchMock = jest.fn(async (url: string) => {
  if (url.endsWith('SHASUMS256.txt')) {
    // Return a minimal SHASUMS file covering the artifacts used in tests.
    return new Response(
      `${FAKE_SHA256}  node-v22.0.0-linux-x64-musl.tar.gz\n`
    )
  }
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
  jest.mocked(isNonGlibcLinux).mockReturnValue(Promise.resolve(false))
  fetchMock.mockClear()
})

test.skip('install Node using a custom node mirror', async () => {
  tempDir()

  const nodeMirrorBaseUrl = 'https://pnpm-node-mirror-test.localhost/download/release/'
  const opts: FetchNodeOptions = {
    nodeMirrorBaseUrl,
    storeDir: path.resolve('store'),
  }

  await fetchNode(fetchMock, '16.4.0', path.resolve('node'), opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch(nodeMirrorBaseUrl)
  }
})

test.skip('install Node using the default node mirror', async () => {
  tempDir()

  const opts: FetchNodeOptions = {
    storeDir: path.resolve('store'),
  }

  await fetchNode(fetchMock, '16.4.0', path.resolve('node'), opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch('https://nodejs.org/download/release/')
  }
})


test('auto-detects musl on non-glibc Linux and uses unofficial-builds mirror', async () => {
  jest.mocked(isNonGlibcLinux).mockReturnValue(Promise.resolve(true))
  tempDir()

  // The function will throw because the downloaded tarball content won't match
  // the fake sha256 we put in the SHASUMS256.txt mock, but all fetch calls are
  // recorded before the integrity check, so we can assert the correct URLs.
  await expect(
    fetchNode(fetchMock, '22.0.0', path.resolve('node'), {
      storeDir: path.resolve('store'),
      platform: 'linux',
      arch: 'x64',
      retry: { retries: 0 },
    })
  ).rejects.toThrow()

  const shasumsUrl = fetchMock.mock.calls[0][0] as string
  expect(shasumsUrl).toContain('unofficial-builds.nodejs.org')

  const tarballUrl = fetchMock.mock.calls[1][0] as string
  expect(tarballUrl).toContain('unofficial-builds.nodejs.org')
  expect(tarballUrl).toContain('node-v22.0.0-linux-x64-musl.tar.gz')
})