import path from 'path'
import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { writeJsonFileSync } from 'write-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpmSync } from './utils/index.js'
import isWindows from 'is-windows'

test('switch to the pnpm version specified in the packageManager field of package.json', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('do not switch to the pnpm version specified in the packageManager field of package.json, if managePackageManagerVersions is set to false', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeYamlFile('pnpm-workspace.yaml', {
    managePackageManagerVersions: false,
  })
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).not.toContain('Version 9.3.0')
})

test('do not switch to pnpm version that is specified not with a semver version', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@kevva/is-positive',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@kevva/is-positive')
})

test('do not switch to pnpm version that is specified starting with v', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@v9.15.5',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@v9.15.5: you need to specify the version as "9.15.5"')
})

test('do not switch to pnpm version when a range is specified', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@^9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@^9.3.0')
})

test('throws error if pnpm binary in store is corrupt', () => {
  prepare()
  const config = ['--config.manage-package-manager-versions=true'] as const
  const pnpmHome = path.resolve('pnpm')
  const storeDir = path.resolve('store')
  const env = { PNPM_HOME: pnpmHome, pnpm_config_store_dir: storeDir }
  const version = '9.3.0'

  writeJsonFileSync('package.json', {
    packageManager: `pnpm@${version}`,
  })

  // Run pnpm once to ensure pnpm is installed to the store.
  execPnpmSync([...config, 'help'], { env })

  // Find the pnpm binary in the global virtual store and corrupt it.
  const entries = fs.readdirSync(storeDir, { recursive: true }) as string[]
  const pnpmBinEntry = entries.find(e => {
    const normalized = e.replace(/\\/g, '/')
    return normalized.endsWith('/bin/pnpm') && !normalized.includes('node_modules')
  })
  if (!pnpmBinEntry) throw new Error('Could not find pnpm binary in store')
  fs.rmSync(path.join(storeDir, pnpmBinEntry))
  if (isWindows()) {
    fs.rmSync(path.join(storeDir, pnpmBinEntry + '.cmd'))
  }

  const { stderr } = execPnpmSync([...config, 'help'], { env })
  expect(stderr.toString()).toContain('Failed to switch pnpm to v9.3.0. Looks like pnpm CLI is missing')
})
