import AdmZip from 'adm-zip'
import { Response } from 'node-fetch'
import path from 'path'
import fs from 'fs'
import { Readable } from 'stream'
import tar from 'tar-stream'
import { globalWarn } from '@pnpm/logger'
import {
  getNodeDir,
  getNodeBinDir,
  getNodeVersionsBaseDir,
  type NvmNodeCommandOptions,
  prepareExecutionEnv,
} from '../lib/node'
import { tempDir } from '@pnpm/prepare'

const fetchMock = jest.fn(async (url: string) => {
  if (url.endsWith('.tar.gz')) {
    const pack = tar.pack()
    pack.finalize()
    return new Response(pack) // pack is a readable stream
  } else if (url.endsWith('.zip')) {
    // The Windows code path for pnpm's node bootstrapping expects a subdir
    // within the .zip file.
    const pkgName = path.basename(url, '.zip')
    const zip = new AdmZip()
    zip.addFile(`${pkgName}/dummy-file`, Buffer.from('test'))

    return new Response(Readable.from(zip.toBuffer()))
  }

  return new Response(Readable.from(Buffer.alloc(0)))
})

jest.mock('@pnpm/fetch', () => ({
  createFetchFromRegistry: () => fetchMock,
}))

jest.mock('@pnpm/logger', () => {
  const originalModule = jest.requireActual('@pnpm/logger')
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

beforeEach(() => {
  fetchMock.mockClear()
  ;(globalWarn as jest.Mock).mockClear()
})

test('check API (placeholder test)', async () => {
  expect(typeof getNodeDir).toBe('function')
})

test('install Node uses node-mirror:release option', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const nodeMirrorRelease = 'https://pnpm-node-mirror-test.localhost/download/release'
  const opts: NvmNodeCommandOptions = {
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {
      'node-mirror:release': nodeMirrorRelease,
    },
    useNodeVersion: '16.4.0',
  }

  await getNodeBinDir(opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch(nodeMirrorRelease)
  }
})

test('install an rc version of Node.js', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const opts: NvmNodeCommandOptions = {
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
    useNodeVersion: 'rc/18.0.0-rc.3',
  }

  await getNodeBinDir(opts)

  const platform = process.platform === 'win32' ? 'win' : process.platform
  const arch = process.arch
  const extension = process.platform === 'win32' ? 'zip' : 'tar.gz'
  expect(fetchMock.mock.calls[0][0]).toBe(
    `https://nodejs.org/download/rc/v18.0.0-rc.3/node-v18.0.0-rc.3-${platform}-${arch}.${extension}`
  )
})

test('get node version base dir', async () => {
  expect(typeof getNodeVersionsBaseDir).toBe('function')
  const versionDir = getNodeVersionsBaseDir(process.cwd())
  expect(versionDir).toBe(path.resolve(process.cwd(), 'nodejs'))
})

test('specified an invalid Node.js via use-node-version should not cause pnpm itself to break', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const opts: NvmNodeCommandOptions = {
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
    useNodeVersion: '22.14',
  }

  fs.mkdirSync('nodejs', { recursive: true })
  fs.writeFileSync('nodejs/versions.json', '{"default":"16.4.0"}', 'utf8')

  expect(await getNodeBinDir(opts)).toBeTruthy()

  const calls = (globalWarn as jest.Mock).mock.calls
  expect(calls[calls.length - 1][0]).toContain('"22.14" is not a valid Node.js version.')
})

describe('prepareExecutionEnv', () => {
  test('should not proceed to fetch Node.js if the process is already running in wanted node version', async () => {
    fetchMock.mockImplementationOnce(() => {
      throw new Error('prepareExecutionEnv should not proceed to fetch Node.js when wanted version is running')
    })

    await prepareExecutionEnv({
      bin: '',
      pnpmHomeDir: process.cwd(),
      rawConfig: {},
    }, {
      executionEnv: { nodeVersion: process.versions.node },
    })
  })
})
