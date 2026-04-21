import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import PATH_NAME from 'path-name'

import { execPnpmSync } from './utils/index.js'

test('pnpm bin', async () => {
  tempDir()
  fs.mkdirSync('node_modules')

  const result = execPnpmSync(['bin'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe(path.resolve('node_modules/.bin'))
})

test('pnpm bin -g', async () => {
  tempDir()

  const binDir = path.join(process.cwd(), 'bin')
  const env = {
    PNPM_HOME: process.cwd(),
    [PATH_NAME]: binDir,
  }

  const result = execPnpmSync(['bin', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toEqual(binDir)
})
