import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import { findRepoRoot, parseSelectedProducts, releaseFilterArgs } from '../src/bump.js'

describe('findRepoRoot', () => {
  let dir: string
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-root-'))
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  test('walks up to the directory containing .changeset', () => {
    fs.mkdirSync(path.join(dir, '.changeset'))
    const nested = path.join(dir, 'pnpm11', '__utils__', 'scripts', 'src')
    fs.mkdirSync(nested, { recursive: true })
    expect(findRepoRoot(nested)).toBe(dir)
  })

  test('throws when no .changeset directory exists above', () => {
    expect(() => findRepoRoot(dir)).toThrow(/No \.changeset directory/)
  })
})

describe('parseSelectedProducts', () => {
  test('collects the products named by --release', () => {
    expect(parseSelectedProducts(['--release', 'pnpm11', '--release', 'pnpr']))
      .toEqual(new Set(['pnpm11', 'pnpr']))
  })

  test('is empty when no argument is passed (a bare `pnpm bump` releases everything)', () => {
    expect(parseSelectedProducts([])).toEqual(new Set())
  })

  test('throws on an unknown product', () => {
    expect(() => parseSelectedProducts(['--release', 'bogus'])).toThrow(/Unknown --release product/)
  })

  test('fails closed on a misspelled flag instead of releasing everything', () => {
    expect(() => parseSelectedProducts(['--releases', 'pnpr'])).toThrow(/Unexpected bump argument/)
  })
})

describe('releaseFilterArgs', () => {
  test('releases everything (no filter) when nothing is selected', () => {
    expect(releaseFilterArgs(new Set())).toEqual([])
  })

  test('releases everything (no filter) when all three products are selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm11', 'pnpm', 'pnpr']))).toEqual([])
  })

  test('excludes the unselected alpha products when pnpm11 is selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm11'])))
      .toEqual(['--filter=!pacquet', '--filter=!@pnpm/napi', '--filter=!@pnpm/pnpr'])
    expect(releaseFilterArgs(new Set(['pnpm11', 'pnpr'])))
      .toEqual(['--filter=!pacquet', '--filter=!@pnpm/napi'])
  })

  test('includes only the selected alpha products when pnpm11 is not selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm'])))
      .toEqual(['--filter=pacquet', '--filter=@pnpm/napi'])
    expect(releaseFilterArgs(new Set(['pnpm', 'pnpr'])))
      .toEqual(['--filter=pacquet', '--filter=@pnpm/napi', '--filter=@pnpm/pnpr'])
    expect(releaseFilterArgs(new Set(['pnpr'])))
      .toEqual(['--filter=@pnpm/pnpr'])
  })
})
