import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import { findRepoRoot } from '../src/bump.js'

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
