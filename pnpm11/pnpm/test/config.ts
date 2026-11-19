import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'lockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})

const testOnWindows = isWindows() ? test : test.skip

testOnWindows('fail when PNPM_HOME contains an unresolved Windows environment variable', () => {
  prepare()
  const result = execPnpmSync(['install'], {
    env: { PNPM_HOME: '%PNPM_TEST_UNSET_VARIABLE%\\pnpm' },
  })
  expect(result.status).not.toBe(0)
  const output = `${result.stdout?.toString()}${result.stderr?.toString()}`
  expect(output).toContain('PNPM_HOME contains an unexpanded environment variable: %PNPM_TEST_UNSET_VARIABLE%')
  expect(fs.existsSync('%PNPM_TEST_UNSET_VARIABLE%')).toBeFalsy()
})

test('resolutions in root package.json are used as overrides when no overrides in pnpm-workspace.yaml', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    private: true,
    resolutions: {
      'is-positive': '3.1.0',
    },
  }), 'utf8')

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const result = execPnpmSync(['install'])
  expect(result.status).toBe(0)
  const stderr = result.stderr.toString()
  expect(stderr).toContain('The "resolutions" field in package.json is deprecated')
  expect(stderr).toContain('We attempted to migrate your resolutions to pnpm overrides')
  expect(stderr).toContain('is-positive: 3.1.0')

  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as any // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-positive': '3.1.0',
  })
})

test('resolutions in root package.json are used as overrides without a pnpm-workspace.yaml', async () => {
  prepare()

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'standalone',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    resolutions: {
      'is-positive': '3.1.0',
    },
  }), 'utf8')

  const result = execPnpmSync(['install'])
  expect(result.status).toBe(0)
  const stderr = result.stderr.toString()
  expect(stderr).toContain('The "resolutions" field in package.json is deprecated')
  expect(stderr).toContain('We attempted to migrate your resolutions to pnpm overrides')
  expect(stderr).toContain('is-positive: 3.1.0')

  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as any // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-positive': '3.1.0',
  })
})

test('warns and drops resolutions when both resolutions and overrides exist', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    private: true,
    resolutions: {
      'is-positive': '3.1.0',
    },
  }), 'utf8')

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    overrides: {
      'is-negative': '1.0.0',
    },
  })

  const result = execPnpmSync(['install'])
  expect(result.status).toBe(0)
  const stderr = result.stderr.toString()
  expect(stderr).toContain('"resolutions" field in package.json is ignored because "overrides" in pnpm-workspace.yaml takes precedence')
  expect(stderr).not.toContain('We attempted to migrate your resolutions to pnpm overrides')

  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as any // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-negative': '1.0.0',
  })
  expect(lockfile.overrides).not.toHaveProperty('is-positive')
})
