import { detectIfCurrentPkgIsExecutable, getCurrentPackageName, isExecutedByCorepack } from '@pnpm/cli-meta'

describe('detectIfCurrentPkgIsExecutable()', () => {
  test('returns false when not running as a SEA binary', () => {
    // In a test environment node:sea is unavailable, so the require() inside
    // detectIfCurrentPkgIsExecutable() throws and the catch block returns false.
    expect(detectIfCurrentPkgIsExecutable()).toBe(false)
  })
})

describe('getCurrentPackageName()', () => {
  test('returns "pnpm" when not running as a SEA binary', () => {
    expect(getCurrentPackageName()).toBe('pnpm')
  })
})

describe('isExecutedByCorepack()', () => {
  test('returns true when COREPACK_ROOT is set', () => {
    expect(isExecutedByCorepack({ COREPACK_ROOT: '/usr/local/lib/corepack' })).toBe(true)
  })

  test('returns false when COREPACK_ROOT is not set', () => {
    expect(isExecutedByCorepack({})).toBe(false)
  })

  test('returns false when COREPACK_ROOT is undefined', () => {
    expect(isExecutedByCorepack({ COREPACK_ROOT: undefined })).toBe(false)
  })
})
