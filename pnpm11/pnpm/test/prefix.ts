import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test('pnpm prefix', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  const result = execPnpmSync(['prefix'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.resolve('.') + '\n')
})

test('pnpm prefix inside a subdirectory', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')
  fs.mkdirSync('sub')
  const originalCwd = process.cwd()
  process.chdir('sub')

  try {
    const result = execPnpmSync(['prefix'])

    expect(result.status).toBe(0)
    expect(result.stdout.toString()).toBe(originalCwd + '\n')
  } finally {
    process.chdir(originalCwd)
  }
})

test('pnpm prefix -g', async () => {
  tempDir()
  const result = execPnpmSync(['prefix', '-g'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBeTruthy()
})
