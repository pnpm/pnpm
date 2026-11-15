import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
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

test('pnpm with current dispatches the inner command after a global boolean flag', () => {
  prepare()
  writeJsonFileSync('package.json', {
    name: 'project',
    version: '1.0.0',
  })

  for (const flag of ['--color', '--yes']) {
    const { status, stdout } = execPnpmSync([flag, 'with', 'current', '--version'])

    expect(status).toBe(0)
    expect(stdout.toString().trim()).toMatch(/^\d+\.\d+\.\d+/)
  }
})

test('pnpm with current composes with pm prefix and subcommand boolean flags', () => {
  prepare()
  writeJsonFileSync('package.json', {
    name: 'project',
    version: '1.0.0',
    packageManager: 'yarn@4.0.0',
    scripts: {
      clean: 'echo script-clean-ran',
    },
  })

  const listResult = execPnpmSync(['--dir', '.', 'with', 'current', 'pm', 'list'])
  expect(listResult.status).toBe(0)

  const newPkgDir = path.resolve('new-pkg')
  fs.mkdirSync(newPkgDir, { recursive: true })
  const initResult = execPnpmSync(['--dir', newPkgDir, '--bare', 'with', 'current', 'init'])
  expect(initResult.status).toBe(0)
  const created = JSON.parse(fs.readFileSync(path.join(newPkgDir, 'package.json'), 'utf8'))
  expect(created.name).toBeUndefined()
  expect(created.version).toBeUndefined()
  expect(created.type).toBe('module')
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
