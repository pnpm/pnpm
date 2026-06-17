import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare, preparePackages } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'lockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
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
  expect(result.stderr.toString()).toContain('The "resolutions" field in package.json is deprecated')

  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as any // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-positive': '3.1.0',
  })
})

test('error when both resolutions and overrides exist', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
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
  expect(result.status).not.toBe(0)
  expect(result.stderr.toString()).toContain('resolutions" field in package.json conflicts with "overrides" in pnpm-workspace.yaml')
})

test('--ignore-resolutions-conflict allows install when both resolutions and overrides exist', async () => {
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

  const result = execPnpmSync(['install', '--ignore-resolutions-conflict'])
  expect(result.status).toBe(0)

  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as any // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-negative': '1.0.0',
  })
  expect(lockfile.overrides).not.toHaveProperty('is-positive')
})
