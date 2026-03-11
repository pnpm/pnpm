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
  expect(stderr.toString()).toContain('This project is configured to use v0.0.0 of pnpm. Your current pnpm is')
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

test('--version should work even if the required package manager is not pnpm', async () => {
  prepare({
    name: 'project',
    packageManager: 'yarn@3.0.0',
    version: '1.0.0',
  })

  const { status, stdout } = execPnpmSync(['--version'])
  expect(status).toBe(0)
  expect(stdout.toString()).toMatch(/^\d+\.\d+\.\d+/)
})

test('--help should work even if the required package manager is not pnpm', async () => {
  prepare({
    name: 'project',
    packageManager: 'yarn@3.0.0',
    version: '1.0.0',
  })

  const { status, stdout } = execPnpmSync(['--help'])
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Usage:')
})
