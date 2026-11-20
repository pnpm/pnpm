import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { preparePackages, tempDir } from '@pnpm/prepare'
import PATH_NAME from 'path-name'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('pnpm root', async () => {
  tempDir()
  fs.writeFileSync('package.json', '{}', 'utf8')

  const result = execPnpmSync(['root'])

  expect(result.status).toBe(0)

  expect(result.stdout.toString()).toBe(path.resolve('node_modules') + '\n')
})

test.each(['vendor', 'www/modules'])('pnpm root prints the configured modules directory %s', async (modulesDir) => {
  tempDir()
  fs.writeFileSync('pnpm-workspace.yaml', `modulesDir: ${modulesDir}\n`, 'utf8')

  const result = execPnpmSync(['root'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe(path.resolve(modulesDir) + '\n')
})

test('pnpm root reports the modules directory a packageConfigs entry gives the project', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'aside', version: '1.0.0' },
    { name: 'plain', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    modulesDir: 'vendor',
    sharedWorkspaceLockfile: false,
    packageConfigs: { aside: { modulesDir: 'private_modules' } },
  })

  const aside = execPnpmSync(['root'], { cwd: path.resolve('aside') })
  expect(aside.status).toBe(0)
  expect(aside.stdout.toString()).toBe(path.resolve('aside/private_modules') + '\n')

  const plain = execPnpmSync(['root'], { cwd: path.resolve('plain') })
  expect(plain.status).toBe(0)
  expect(plain.stdout.toString()).toBe(path.resolve('plain/vendor') + '\n')
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
