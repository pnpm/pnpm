import { Response } from 'node-fetch'
import path from 'path'
import fs from 'fs'
import { Readable } from 'stream'
import tar from 'tar-stream'
import { jest } from '@jest/globals'
import { ZipFile } from 'yazl'
import { tempDir } from '@pnpm/prepare'
import { type NvmNodeCommandOptions } from '../lib/node.js'

const fetchMock = jest.fn(async (url: string) => {
  if (url.endsWith('SHASUMS256.txt')) {
    return new Response(`
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v16.4.0-darwin-arm64.tar.gz
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v16.4.0-linux-arm64.tar.gz
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v16.4.0-linux-x64.tar.gz
a08f3386090e6511772b949d41970b75a6b71d28abb551dff9854ceb1929dae1  node-v16.4.0-win-x64.zip
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v18.0.0-rc.3-darwin-arm64.tar.gz
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v18.0.0-rc.3-linux-arm64.tar.gz
5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef  node-v18.0.0-rc.3-linux-x64.tar.gz
07e6121cba611b57f310a489f76c413b6246e79cffe1e9538b2478ffee11c99e  node-v18.0.0-rc.3-win-x64.zip
`)
  }
  if (url.endsWith('.tar.gz')) {
    const pack = tar.pack()
    pack.finalize()
    return new Response(pack) // pack is a readable stream
  } else if (url.endsWith('.zip')) {
    // The Windows code path for pnpm's node bootstrapping expects a subdir
    // within the .zip file.
    const pkgName = path.basename(url, '.zip')
    const zipfile = new ZipFile()

    zipfile.addBuffer(Buffer.from('test'), `${pkgName}/dummy-file`, {
      mtime: new Date(0), // fixed timestamp for determinism
      mode: 0o100644, // fixed file permissions
    })

    zipfile.end()
    return new Response(Readable.from(zipfile.outputStream))
  }

  return new Response(Readable.from(Buffer.alloc(0)))
})

jest.unstable_mockModule('@pnpm/fetch', () => ({
  createFetchFromRegistry: () => fetchMock,
}))

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

const { globalWarn } = await import('@pnpm/logger')
const {
  getNodeDir,
  getNodeBinDir,
  getNodeVersionsBaseDir,
  prepareExecutionEnv,
} = await import('../lib/node.js')

beforeEach(() => {
  fetchMock.mockClear()
  jest.mocked(globalWarn).mockClear()
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
  expect(fetchMock.mock.calls[1][0]).toBe(
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

  const calls = jest.mocked(globalWarn).mock.calls
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
