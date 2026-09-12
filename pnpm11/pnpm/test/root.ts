import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import PATH_NAME from 'path-name'

import { execPnpmSync } from './utils/index.js'

test('pnpm root', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  const result = execPnpmSync(['root'])

  expect(result.status).toBe(0)

  expect(result.stdout.toString()).toBe(path.resolve('node_modules') + '\n')
})

test('pnpm root -g', async () => {
  tempDir()

  const global = path.resolve('global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = { [PATH_NAME]: path.join(pnpmHome, 'bin'), PNPM_HOME: pnpmHome, XDG_DATA_HOME: global }

  const result = execPnpmSync(['root', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.join(global, `pnpm/global/${GLOBAL_LAYOUT_VERSION}`) + '\n')
})

test('pnpm root -g writes warnings to stderr so stdout stays a clean path', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({ packageManager: 'pnpm@0.0.0' }), 'utf8')

  const global = path.resolve('global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: path.join(pnpmHome, 'bin'),
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
    COREPACK_ROOT: '/fake/corepack',
  }

  const result = execPnpmSync(['root', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.join(global, `pnpm/global/${GLOBAL_LAYOUT_VERSION}`) + '\n')
  expect(result.stderr.toString()).toContain('Using --global skips the package manager check for this project')
})
