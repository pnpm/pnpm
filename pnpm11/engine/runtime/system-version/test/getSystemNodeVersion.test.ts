import { expect, jest, test } from '@jest/globals'
let isSea = false

jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  detectIfCurrentPkgIsExecutable: jest.fn(() => isSea),
}))

jest.unstable_mockModule('execa', () => ({
  sync: jest.fn(() => ({
    stdout: 'v10.0.0',
  })),
}))

const {
  getSystemNodeVersionNonCached,
  getSystemDenoVersionNonCached,
  getSystemBunVersionNonCached,
  getSystemRuntimeVersion,
  engineName,
} = await import('../lib/index.js')
const execa = await import('execa')

test('getSystemNodeVersion() executed from an executable pnpm CLI', () => {
  isSea = true
  expect(getSystemNodeVersionNonCached()).toBe('v10.0.0')
  expect(execa.sync).toHaveBeenCalledWith('node', ['--version'])
})

test('getSystemNodeVersion() from a non-executable pnpm CLI', () => {
  isSea = false
  expect(getSystemNodeVersionNonCached()).toBe(process.version)
})

test('getSystemNodeVersion() returns undefined if execa.sync throws an error', () => {
  // Mock execa.sync to throw an error
  jest.mocked(execa.sync).mockImplementationOnce(() => {
    throw new Error('not found: node')
  })

  isSea = true
  expect(getSystemNodeVersionNonCached()).toBeUndefined()
  expect(execa.sync).toHaveBeenCalledWith('node', ['--version'])
})

test('getSystemDenoVersion() parses the first line of `deno --version`', () => {
  jest.mocked(execa.sync).mockReturnValueOnce({
    stdout: 'deno 1.40.0 (release, aarch64-apple-darwin)\nv8 12.1.285.27\ntypescript 5.3.3',
  } as ReturnType<typeof execa.sync>)
  expect(getSystemDenoVersionNonCached()).toBe('v1.40.0')
  expect(execa.sync).toHaveBeenCalledWith('deno', ['--version'])
})

test('getSystemDenoVersion() returns undefined when deno is missing or output is unexpected', () => {
  jest.mocked(execa.sync).mockImplementationOnce(() => {
    throw new Error('not found: deno')
  })
  expect(getSystemDenoVersionNonCached()).toBeUndefined()

  jest.mocked(execa.sync).mockReturnValueOnce({ stdout: 'unexpected output' } as ReturnType<typeof execa.sync>)
  expect(getSystemDenoVersionNonCached()).toBeUndefined()
})

test('getSystemBunVersion() parses the bare version printed by `bun --version`', () => {
  jest.mocked(execa.sync).mockReturnValueOnce({ stdout: '1.1.0\n' } as ReturnType<typeof execa.sync>)
  expect(getSystemBunVersionNonCached()).toBe('v1.1.0')
  expect(execa.sync).toHaveBeenCalledWith('bun', ['--version'])
})

test('getSystemBunVersion() returns undefined when bun is missing', () => {
  jest.mocked(execa.sync).mockImplementationOnce(() => {
    throw new Error('not found: bun')
  })
  expect(getSystemBunVersionNonCached()).toBeUndefined()
})

test('getSystemRuntimeVersion() dispatches to the per-runtime helpers', () => {
  isSea = false
  expect(getSystemRuntimeVersion('node')).toBe(process.version)

  jest.mocked(execa.sync).mockReturnValueOnce({
    stdout: 'deno 9.9.9 (release)',
  } as ReturnType<typeof execa.sync>)
  expect(getSystemRuntimeVersion('deno')).toBe('v9.9.9')
  expect(execa.sync).toHaveBeenLastCalledWith('deno', ['--version'])

  jest.mocked(execa.sync).mockReturnValueOnce({
    stdout: '9.9.9\n',
  } as ReturnType<typeof execa.sync>)
  expect(getSystemRuntimeVersion('bun')).toBe('v9.9.9')
  expect(execa.sync).toHaveBeenLastCalledWith('bun', ['--version'])
})

test('engineName() honours an explicit nodeVersion over the host probe', () => {
  // The pinned-runtime override path: when a project's
  // `engines.runtime` / `devEngines.runtime` resolves to a specific
  // Node version, the caller forwards it to `engineName(version)`
  // and the result reflects that pinned Node — not whatever pnpm
  // itself is running on. Format-stable across `v`-prefixed and
  // bare versions.
  const major22 = `${process.platform};${process.arch};node22`
  expect(engineName('22.11.0')).toBe(major22)
  expect(engineName('v22.11.0')).toBe(major22)
})

test('engineName() falls back to the host Node when no override is provided', () => {
  // No-arg call mirrors the pre-runtime-pin behaviour: anchor to
  // `getSystemNodeVersion()` (which itself prefers shell `node` over
  // `process.version` only when running as a SEA bundle — covered
  // by the tests above). Non-SEA test environment, so the system
  // version equals `process.version`.
  isSea = false
  const major = process.version.replace(/^v/, '').split('.')[0]
  expect(engineName()).toBe(`${process.platform};${process.arch};node${major}`)
})
