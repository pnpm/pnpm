import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test('install should fail if the used pnpm version does not satisfy the pnpm version specified in engines', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    engines: {
      pnpm: '99999',
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your pnpm version is incompatible with')
})

test('install should not fail if the used pnpm version does not satisfy the pnpm version specified in packageManager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'pnpm@0.0.0',
  })

  expect(execPnpmSync(['install', '--config.manage-package-manager-versions=false']).status).toBe(0)

  const { status, stderr } = execPnpmSync(['install', '--config.manage-package-manager-versions=false', '--config.package-manager-strict-version=true'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.0 of pnpm. Your current pnpm is')
})

test('install should fail if the project requires a different package manager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'yarn@4.0.0',
  })

  const { status, stderr } = execPnpmSync(['install', '--config.manage-package-manager-versions=true'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use yarn')

  expect(execPnpmSync(['install', '--config.package-manager-strict=false']).status).toBe(0)
})

test('install should not fail for packageManager field with hash', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()

  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: `pnpm@${pnpmVersion}+sha256.123456789`,
  })

  const { status } = execPnpmSync(['install'])
  expect(status).toBe(0)
})

test('install should not fail for packageManager field with url', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'pnpm@https://github.com/pnpm/pnpm',
  })

  const { status } = execPnpmSync(['install'])
  expect(status).toBe(0)
})

test('some commands should not fail if the required package manager is not pnpm', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'yarn@3.0.0',
  })

  const { status } = execPnpmSync(['store', 'path'])
  expect(status).toBe(0)
})

test('devEngines.packageManager with onFail=error should fail on version mismatch', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager with onFail=warn should warn on version mismatch', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'warn',
      },
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager with onFail=ignore should not check version', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'ignore',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stdout.toString()).not.toContain('0.0.1')
  expect(stderr.toString()).not.toContain('0.0.1')
})

test('devEngines.packageManager defaults to onFail=error', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager with a different PM name should fail with onFail=error', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'yarn',
        version: '>=4.0.0',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use yarn')
})

test('devEngines.packageManager array selects the pnpm entry', async () => {
  prepare({
    devEngines: {
      packageManager: [
        { name: 'yarn', version: '>=4.0.0', onFail: 'ignore' },
        { name: 'pnpm', version: '0.0.1', onFail: 'error' },
      ],
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager array defaults onFail to ignore for non-last elements', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()
  prepare({
    devEngines: {
      packageManager: [
        { name: 'pnpm', version: pnpmVersion },
        { name: 'yarn', version: '>=4.0.0' },
      ],
    },
  })

  // pnpm is the first (non-last) element, so onFail defaults to 'ignore'
  const { status } = execPnpmSync(['install'])

  expect(status).toBe(0)
})

test('devEngines.packageManager with version range should match current version', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=1.0.0',
        onFail: 'error',
      },
    },
  })

  const { status } = execPnpmSync(['install'])

  expect(status).toBe(0)
})

test('devEngines.packageManager takes precedence over packageManager field', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()
  prepare({
    packageManager: `pnpm@${pnpmVersion}`,
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  // devEngines.packageManager takes effect, so version mismatch error is thrown
  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})
