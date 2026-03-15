import fs from 'node:fs'
import path from 'node:path'

import { prepare } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { writeJsonFileSync } from 'write-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

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
  writeYamlFileSync('pnpm-workspace.yaml', {
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

  const { stderr } = execPnpmSync(['help'], { env })

  expect(stderr.toString()).toContain('"kevva/is-positive" is not a valid exact version')
})

test('do not switch to pnpm version that is specified starting with v', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@v9.15.5',
  })

  const { stderr } = execPnpmSync(['help'], { env })

  expect(stderr.toString()).toContain('you need to specify the version as "9.15.5"')
})

test('do not switch to pnpm version when a range is specified in packageManager field', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@^9.3.0',
  })

  const { stderr } = execPnpmSync(['help'], { env })

  expect(stderr.toString()).toContain('not a valid exact version')
})

test('switch to the pnpm version resolved from devEngines.packageManager with onFail=download', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '9.3.0',
        onFail: 'download',
      },
    },
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('switch to the pnpm version resolved from devEngines.packageManager with a range', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=9.1.0 <9.1.4',
        onFail: 'download',
      },
    },
  })

  const { stdout } = execPnpmSync(['help'], { env })

  // Should resolve to the highest version in the range (9.1.3, not 9.1.0)
  expect(stdout.toString()).toContain('Version 9.1.3')
})

test('devEngines.packageManager with onFail=download reuses resolved version from env lockfile', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=9.1.0 <9.1.4',
        onFail: 'download',
      },
    },
  })

  // First run: resolves and writes env lockfile
  const firstRun = execPnpmSync(['help'], { env })
  expect(firstRun.stdout.toString()).toContain('Version 9.1.3')

  // Second run: should reuse the resolved version from env lockfile
  const secondRun = execPnpmSync(['help'], { env })
  expect(secondRun.stdout.toString()).toContain('Version 9.1.3')

  // Verify env lockfile was written
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(true)
})

test('devEngines.packageManager re-resolves when locked version no longer satisfies updated range', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }

  // First run: seed the lockfile with 9.1.1
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=9.1.0 <9.1.2',
        onFail: 'download',
      },
    },
  })
  const firstRun = execPnpmSync(['help'], { env })
  expect(firstRun.stdout.toString()).toContain('Version 9.1.1')
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(true)

  // Update range so the previously locked 9.1.1 no longer satisfies it
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=9.1.2 <9.1.4',
        onFail: 'download',
      },
    },
  })

  // Should re-resolve and switch to 9.1.3
  const secondRun = execPnpmSync(['help'], { env })
  expect(secondRun.stdout.toString()).toContain('Version 9.1.3')
})

test('devEngines.packageManager without onFail=download does not switch version', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeYamlFileSync('pnpm-workspace.yaml', {
    managePackageManagerVersions: false,
  })
  writeJsonFileSync('package.json', {
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '9.3.0',
        onFail: 'error',
      },
    },
  })

  const { status, stdout } = execPnpmSync(['help'], { env })

  expect(status).not.toBe(0)
  expect(stdout.toString()).not.toContain('Version 9.3.0')
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
