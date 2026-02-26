import { jest } from '@jest/globals'

let isSea = false

jest.unstable_mockModule('@pnpm/cli-meta', () => ({
  detectIfCurrentPkgIsExecutable: jest.fn(() => isSea),
}))

jest.unstable_mockModule('execa', () => ({
  sync: jest.fn(() => ({
    stdout: 'v10.0.0',
  })),
}))

const { getSystemNodeVersionNonCached } = await import('../lib/index.js')
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
