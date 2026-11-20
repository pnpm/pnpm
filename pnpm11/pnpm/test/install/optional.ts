import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import deepRequireCwd from 'deep-require-cwd'

import { execPnpm, execPnpmSync } from '../utils/index.js'

test('installing optional dependencies when --no-optional is not used', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install'])

  project.has('is-positive')
  project.has('@pnpm.e2e/pkg-with-good-optional')

  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-good-optional', '@pnpm.e2e/dep-of-pkg-with-1-dep', './package.json'])).toBeTruthy()
  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-good-optional', 'is-positive', './package.json'])).toBeTruthy()
})

test('not installing optional dependencies when --no-optional is used', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install', '--no-optional'])

  project.hasNot('is-positive')
  project.has('@pnpm.e2e/pkg-with-good-optional')

  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-good-optional', '@pnpm.e2e/dep-of-pkg-with-1-dep', './package.json'])).toBeTruthy()
  expect(deepRequireCwd.silent(['@pnpm.e2e/pkg-with-good-optional', 'is-positive', './package.json'])).toBeFalsy()
})

// https://github.com/pnpm/pnpm/issues/16514
test('a frozen install warns about an optional dependency that cannot be fetched', async () => {
  const project = prepare({
    dependencies: {
      'is-negative': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })
  await execPnpm(['install', '--lockfile-only'])
  const lockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  const integrityStart = lockfile.indexOf('sha512-', lockfile.indexOf('  is-positive@1.0.0:\n'))
  const integrityEnd = lockfile.indexOf('}', integrityStart)
  fs.writeFileSync('pnpm-lock.yaml', `${lockfile.slice(0, integrityStart)}sha512-${'A'.repeat(86)}==${lockfile.slice(integrityEnd)}`)

  const { status, stdout } = execPnpmSync(['install', '--frozen-lockfile', '--config.fetch-retries=0'])

  expect(status).toBe(0)
  const output = stdout.toString()
  expect(output).toContain('is-positive@1.0.0 is an optional dependency that could not be fetched. Excluding it from installation.')
  expect(output).toContain('ERR_PNPM_TARBALL_INTEGRITY')
  expect(output).not.toContain('+ is-positive')
  project.has('is-negative')
  expect(fs.lstatSync('node_modules/is-positive', { throwIfNoEntry: false })).toBeUndefined()
})
