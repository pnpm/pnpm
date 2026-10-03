import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { detectIfCurrentPkgIsExecutable, getCurrentPackageName, homebrewFormulaOf, isExecutedByCorepack } from '@pnpm/cli.meta'

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

describe('homebrewFormulaOf()', () => {
  const fromSegments = (...segments: string[]) => path.join(path.sep, ...segments)

  test('returns the formula whose keg holds the path', () => {
    expect(homebrewFormulaOf(fromSegments('opt', 'homebrew', 'Cellar', 'pnpm', '12.8.1', 'bin', 'pnpm'))).toBe('pnpm')
    expect(homebrewFormulaOf(fromSegments('usr', 'local', 'Cellar', 'pnpm@11', '11.2.0', 'lib', 'node_modules', 'pnpm', 'dist', 'pnpm.mjs'))).toBe('pnpm@11')
  })

  test('returns undefined outside a pnpm keg', () => {
    expect(homebrewFormulaOf(fromSegments('opt', 'homebrew', 'Cellar', 'node', '24.0.0', 'bin', 'pnpm'))).toBeUndefined()
    expect(homebrewFormulaOf(fromSegments('opt', 'homebrew', 'Cellar', 'pnpm'))).toBeUndefined()
    expect(homebrewFormulaOf(fromSegments('home', 'user', '.local', 'share', 'pnpm', 'pnpm'))).toBeUndefined()
  })
})
