import fs from 'fs'
import path from 'path'
import { jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const mockDownloadNodeVersion = jest.fn<(opts: any, version: string) => Promise<any>>()
jest.unstable_mockModule('@pnpm/plugin-commands-env', () => ({
  downloadNodeVersion: mockDownloadNodeVersion,
}))

const mockExecaSync = jest.fn()
jest.unstable_mockModule('execa', () => ({
  default: { sync: mockExecaSync },
  sync: mockExecaSync,
}))

const { handler } = await import('../lib/buildSea.js')

// A stable fake nodeDir — no binary needs to actually exist since execa is mocked.
const FAKE_NODE_DIR = path.join(path.sep, 'tmp', 'pnpm-test-node')

beforeEach(() => {
  mockDownloadNodeVersion.mockClear()
  mockExecaSync.mockClear()
  mockDownloadNodeVersion.mockResolvedValue({
    nodeVersion: '22.0.0',
    nodeDir: FAKE_NODE_DIR,
    nodeMirrorBaseUrl: 'https://nodejs.org',
  })
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
      handler(baseOpts() as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_ENTRY' })
  })

  test('throws ENTRY_NOT_FOUND when the entry file does not exist', async () => {
    tempDir()
    await expect(
      handler({ ...baseOpts(), entry: 'nonexistent.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_ENTRY_NOT_FOUND' })
  })

  test('throws MISSING_TARGET when no --target is given', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_TARGET' })
  })

  test('throws INVALID_TARGET for unknown OS (freebsd)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'freebsd-x64' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })

  test('throws INVALID_TARGET for unknown arch (mips)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-mips' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })

  test('throws INVALID_TARGET for incomplete target (os only)', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')
    await expect(
      handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_TARGET' })
  })
})

describe('target parsing', () => {
  test('maps macos to darwin platform', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'macos-arm64' } as any, [])

    const targetCall = mockDownloadNodeVersion.mock.calls.find(
      ([opts]) => (opts as any).platform === 'darwin'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![0]).toMatchObject({ platform: 'darwin', arch: 'arm64' })
  })

  test('maps win to win32 platform and uses .exe extension', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({
      ...baseOpts(),
      entry: 'entry.cjs',
      target: 'win-x64',
      outputName: 'mytool',
    } as any, [])

    const targetCall = mockDownloadNodeVersion.mock.calls.find(
      ([opts]) => (opts as any).platform === 'win32'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![0]).toMatchObject({ platform: 'win32', arch: 'x64', libc: undefined })
    expect(result).toContain('mytool.exe')
  })

  test('passes libc: musl for linux-x64-musl target', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64-musl' } as any, [])

    const targetCall = mockDownloadNodeVersion.mock.calls.find(
      ([opts]) => (opts as any).platform === 'linux'
    )
    expect(targetCall).toBeDefined()
    expect(targetCall![0]).toMatchObject({ platform: 'linux', arch: 'x64', libc: 'musl' })
  })

  test('passes the specified node version to downloadNodeVersion', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64', nodeVersion: '20' } as any, [])

    const targetCall = mockDownloadNodeVersion.mock.calls.find(
      ([opts]) => (opts as any).platform === 'linux'
    )
    expect(targetCall![1]).toBe('20')
  })
})

describe('SEA invocation', () => {
  test('calls node --build-sea with a JSON config file', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64' } as any, [])

    const seaCall = mockExecaSync.mock.calls.find(
      ([, args]) => (args as string[]).includes('--build-sea')
    )
    expect(seaCall).toBeDefined()
    expect(seaCall![1]).toEqual(['--build-sea', expect.stringMatching(/\.json$/)])
  })

  test('places output in dist-sea/<target>/<name> by default', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({ ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64' } as any, [])

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
    } as any, [])

    expect(result).toContain(path.join('release', 'linux-x64', 'myapp'))
  })

  test('builds multiple targets and reports the count', async () => {
    tempDir()
    fs.writeFileSync('entry.cjs', 'module.exports = {}')

    const result = await handler({
      ...baseOpts(),
      entry: 'entry.cjs',
      target: ['linux-x64', 'win-x64', 'macos-arm64'],
    } as any, [])

    expect(result).toContain('3 executables')
    // execa.sync must have been called once per target (for --build-sea)
    const seaCalls = mockExecaSync.mock.calls.filter(
      ([, args]) => (args as string[]).includes('--build-sea')
    )
    expect(seaCalls).toHaveLength(3)
  })
})
