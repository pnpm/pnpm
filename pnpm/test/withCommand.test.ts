import path from 'node:path'

import { prepare } from '@pnpm/prepare'
import { writeJsonFileSync } from 'write-json-file'

import { execPnpmSync } from './utils/index.js'

test('pnpm with current runs the currently active pnpm even when the project pins a different version', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { status, stdout } = execPnpmSync(['with', 'current', 'help'], { env })

  expect(status).toBe(0)
  expect(stdout.toString()).not.toContain('Version 9.3.0')
})

test('pnpm with current bypasses the packageManager check when an unrelated package manager is pinned', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'yarn@4.0.0',
  })

  const { status, stderr } = execPnpmSync(['with', 'current', 'help'], { env })

  expect(status).toBe(0)
  expect(stderr.toString()).not.toContain('This project is configured to use yarn')
})

test('pnpm with current bypasses devEngines.packageManager with onFail=download', () => {
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

  const { status, stdout } = execPnpmSync(['with', 'current', 'help'], { env })

  expect(status).toBe(0)
  expect(stdout.toString()).not.toContain('Version 9.3.0')
})

test('pnpm with forwards subsequent args to the child pnpm', () => {
  prepare()
  writeJsonFileSync('package.json', {
    name: 'project',
    version: '1.0.0',
  })

  const { status, stdout } = execPnpmSync(['with', 'current', '--version'])

  expect(status).toBe(0)
  expect(stdout.toString().trim()).toMatch(/^\d+\.\d+\.\d+/)
})

test('pnpm with fails when no spec is provided', () => {
  prepare()

  const { status, stderr } = execPnpmSync(['with'])

  expect(status).not.toBe(0)
  expect(stderr.toString()).toContain('Missing version argument')
})

test('pnpm with <version> downloads and runs the specified pnpm version', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }

  const { status, stdout } = execPnpmSync(['with', '9.3.0', 'help'], { env })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('pnpm with <version> ignores the packageManager pin and uses the requested version', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.1.0',
  })

  const { status, stdout } = execPnpmSync(['with', '9.3.0', 'help'], { env })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Version 9.3.0')
  expect(stdout.toString()).not.toContain('Version 9.1.0')
})
