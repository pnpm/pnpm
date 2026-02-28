import fs from 'fs'
import path from 'path'
import { jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import type { BuildSeaOptions } from '../lib/buildSea.js'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const mockFetchNode = jest.fn<(...args: any[]) => Promise<void>>().mockResolvedValue(undefined)
jest.unstable_mockModule('@pnpm/node.fetcher', () => ({
  fetchNode: mockFetchNode,
}))

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const mockResolveNodeVersion = jest.fn<(...args: any[]) => Promise<string>>().mockResolvedValue('22.0.0')
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const mockParseNodeSpecifier = jest.fn<(...args: any[]) => unknown>().mockImplementation((specifier: string) => ({
  releaseChannel: 'release',
  versionSpecifier: specifier,
}))
const mockGetNodeMirror = jest.fn().mockReturnValue('https://nodejs.org/download/release/')
jest.unstable_mockModule('@pnpm/node.resolver', () => ({
  resolveNodeVersion: mockResolveNodeVersion,
  resolveNodeVersions: jest.fn(),
  resolveNodeRuntime: jest.fn(),
  parseNodeSpecifier: mockParseNodeSpecifier,
  getNodeMirror: mockGetNodeMirror,
  getNodeArtifactAddress: jest.fn(),
  DEFAULT_NODE_MIRROR_BASE_URL: 'https://nodejs.org/download/release/',
  UNOFFICIAL_NODE_MIRROR_BASE_URL: 'https://unofficial-builds.nodejs.org/download/release/',
}))

const mockExecaSync = jest.fn()
jest.unstable_mockModule('execa', () => ({
  default: { sync: mockExecaSync },
  sync: mockExecaSync,
}))

const { handler } = await import('../lib/buildSea.js')

beforeEach(() => {
  mockFetchNode.mockClear()
  mockExecaSync.mockClear()
  mockResolveNodeVersion.mockClear()
  mockParseNodeSpecifier.mockClear()
  mockFetchNode.mockResolvedValue(undefined)
  mockResolveNodeVersion.mockResolvedValue('22.0.0')
  mockParseNodeSpecifier.mockImplementation((specifier: unknown) => ({
    releaseChannel: 'release',
    versionSpecifier: specifier,
  }))
})

function baseOpts () {
  return {
    dir: process.cwd(),
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }
}

describe('validation', () => {
  test('throws MISSING_ENTRY when --entry is not provided', async () => {
    tempDir()
    await expect(
      handler(baseOpts() as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_ENTRY' })
  })

  test('throws ENTRY_NOT_FOUND when the entry file does not exist', async () => {
    tempDir()
    await expect(
      handler({ ...baseOpts(), entry: 'nonexistent.cjs' } as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_ENTRY_NOT_FOUND' })
  })

  test('throws MISSING_TARGET when no --target is given', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs' } as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_TARGET' })
  })

  test('throws INVALID_TARGET for unknown OS (freebsd)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'freebsd-x64' } as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })

  test('throws INVALID_TARGET for unknown arch (mips)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-mips' } as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })

  test('throws INVALID_TARGET for incomplete target (os only)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux' } as unknown as BuildSeaOptions, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })
})

describe('target parsing', () => {
  test('maps macos to darwin platform', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'macos-arm64' } as unknown as BuildSeaOptions, [])

    const targetCall = mockFetchNode.mock.calls.find(
      ([, , , opts]) => (opts as { platform?: string }).platform === 'darwin'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![3]).toMatchObject({ platform: 'darwin', arch: 'arm64' })
  })

  test('maps win to win32 platform and uses .exe extension', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({
      ...baseOpts(),
      entry: 'entry.cjs',
      target: 'win-x64',
      outputName: 'my-tool',
    } as unknown as BuildSeaOptions, [])

    const targetCall = mockFetchNode.mock.calls.find(
      ([, , , opts]) => (opts as { platform?: string }).platform === 'win32'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![3]).toMatchObject({ platform: 'win32', arch: 'x64', libc: undefined })
    expect(result).toContain('my-tool.exe')
  })

  test('passes libc: musl for linux-x64-musl target', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64-musl' } as unknown as BuildSeaOptions, [])

    const targetCall = mockFetchNode.mock.calls.find(
      ([, , , opts]) => (opts as { platform?: string }).platform === 'linux'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![3]).toMatchObject({ platform: 'linux', arch: 'x64', libc: 'musl' })
  })

  test('passes the specified node version to resolveNodeVersion', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64', nodeVersion: '20' } as unknown as BuildSeaOptions, [])

    const targetCall = mockResolveNodeVersion.mock.calls.find(
      ([, versionSpecifier]) => versionSpecifier === '20'
    )
    expect(targetCall).toBeDefined()
  })
})

describe('SEA invocation', () => {
  test('calls node --build-sea with a JSON config file', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64' } as unknown as BuildSeaOptions, [])

    const seaCall = mockExecaSync.mock.calls.find(
      ([, args]) => (args as string[]).includes('--build-sea')
    )
    expect(seaCall).toBeDefined()
    expect(seaCall![1]).toEqual(['--build-sea', expect.stringMatching(/\.json$/)])
  })

  test('places output in dist-sea/<target>/<name> by default', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64' } as unknown as BuildSeaOptions, [])

    expect(result).toContain(path.join('dist-sea', 'linux-x64'))
  })

  test('respects --output-dir and --output-name', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({
      ...baseOpts(),
      entry: 'entry.cjs',
      target: 'linux-x64',
      outputDir: 'release',
      outputName: 'myapp',
    } as unknown as BuildSeaOptions, [])

    expect(result).toContain(path.join('release', 'linux-x64', 'myapp'))
  })

  test('builds multiple targets and reports the count', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({
      ...baseOpts(),
      entry: 'entry.cjs',
      target: ['linux-x64', 'win-x64', 'macos-arm64'],
    } as unknown as BuildSeaOptions, [])

    expect(result).toContain('3 executables')
    // execa.sync must have been called once per target (for --build-sea)
    const seaCalls = mockExecaSync.mock.calls.filter(
      ([, args]) => (args as string[]).includes('--build-sea')
    )
    expect(seaCalls).toHaveLength(3)
  })
})
