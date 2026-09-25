import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages, tempDir } from '@pnpm/prepare'
import PATH_NAME from 'path-name'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('pnpm bin', async () => {
  tempDir()
  fs.mkdirSync('node_modules')

  const result = execPnpmSync(['bin'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe(path.resolve('node_modules/.bin'))
})

test('pnpm bin prints the configured modules directory', async () => {
  tempDir()
  fs.writeFileSync('pnpm-workspace.yaml', 'modulesDir: vendor\n', 'utf8')

  const result = execPnpmSync(['bin'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe(path.resolve('vendor/.bin'))
})

test('pnpm bin reports the modules directory a packageConfigs entry gives the project', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'moved', version: '1.0.0' },
    { name: 'plain', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    modulesDir: 'vendor',
    sharedWorkspaceLockfile: false,
    packageConfigs: { moved: { modulesDir: 'node_modules' } },
  })

  const moved = execPnpmSync(['bin'], { cwd: path.resolve('moved') })
  expect(moved.status).toBe(0)
  expect(moved.stdout.toString().trim()).toBe(path.resolve('moved/node_modules/.bin'))

  const envGlobal = execPnpmSync(['bin'], {
    cwd: path.resolve('moved'),
    env: { ...process.env, PNPM_CONFIG_GLOBAL: 'true' },
  })
  expect(envGlobal.status).toBe(0)
  expect(envGlobal.stdout.toString().trim()).toBe(path.resolve('moved/node_modules/.bin'))

  const plain = execPnpmSync(['bin'], { cwd: path.resolve('plain') })
  expect(plain.status).toBe(0)
  expect(plain.stdout.toString().trim()).toBe(path.resolve('plain/vendor/.bin'))
})

test('pnpm bin ignores a packageConfigs entry when the workspace shares one lockfile', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'moved', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    modulesDir: 'vendor',
    packageConfigs: { moved: { modulesDir: 'node_modules' } },
  })

  const result = execPnpmSync(['bin'], { cwd: path.resolve('moved') })

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toBe(path.resolve('moved/vendor/.bin'))
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

test('pnpm bin -g writes warnings to stderr so stdout stays a clean path', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({ packageManager: 'pnpm@0.0.0' }), 'utf8')

  const binDir = path.join(process.cwd(), 'bin')
  const env = {
    PNPM_HOME: process.cwd(),
    [PATH_NAME]: binDir,
  }

  const result = execPnpmSync(['--pm-on-fail=warn', 'bin', '-g'], { env })

  expect(result.status).toBe(0)
  expect(result.stdout.toString().trim()).toEqual(binDir)
  expect(result.stderr.toString()).toContain('Using --global skips the package manager check for this project')
})
