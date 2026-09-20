import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import type { PackageManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

const basicPackageManifest = loadJsonFileSync<PackageManifest>(path.join(import.meta.dirname, '../utils/simple-package.json'))

test('production install (with --production flag)', async () => {
  const project = prepare(basicPackageManifest)

  await execPnpm(['install', '--production'])

  project.hasNot(Object.keys(basicPackageManifest.devDependencies!)[0])
  project.has('rimraf')
  project.has('is-positive')
})

test('install dev dependencies only', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '^1.0.0',
    },
    devDependencies: {
      'is-negative': '^1.0.0',
    },
  })

  // NODE_ENV should be ignored if --only is used
  const originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'

  await execPnpm(['install', '--only', 'dev'])

  // reset NODE_ENV
  process.env.NODE_ENV = originalNodeEnv

  const isNegative = project.requireModule('is-negative')
  expect(typeof isNegative).toBe('function')

  project.hasNot('is-positive')
})

const PROD_DIRECT = '@pnpm.e2e/has-foo-100.1.0-dep-1@1.0.0'
const SHARED = '@pnpm.e2e/foo@100.1.0'
const DEV_DIRECT = '@pnpm.e2e/bravo@1.0.0'
const DEV_TRANSITIVE = '@pnpm.e2e/bravo-dep@1.1.0'

function storeHolds (pkg: string): boolean {
  return execPnpmSync(['cat-index', pkg]).status === 0
}

test('production install does not download a package only a devDependency reaches', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/has-foo-100.1.0-dep-1': '1.0.0',
    },
    devDependencies: {
      // Reached by the production dependency above as well, so a prod
      // install still downloads it.
      '@pnpm.e2e/foo': '100.1.0',
      '@pnpm.e2e/bravo': '1.0.0',
    },
  })

  const { stdout } = execPnpmSync(['install', '--prod'], {
    env: { pnpm_config_silent: 'false' },
    stdio: 'pipe',
    expectSuccess: true,
  })
  const output = stdout.toString()

  // The resolve pass runs before materialization, so it must not report the
  // install finished, nor claim a summary, before the packages are fetched
  // and linked (pnpm/pnpm#881). Both passes feed one progress line, so
  // neither may count the same package twice: this graph has 4 packages, 2
  // of them production.
  const progress = /Progress: resolved (\d+), reused \d+, downloaded (\d+), added (\d+), done/.exec(output)
  expect(progress?.slice(1).map(Number)).toStrictEqual([4, 2, 2])
  expect(output).toContain('dependencies:')
  expect(output).toContain('+ @pnpm.e2e/has-foo-100.1.0-dep-1')

  // The absence assertions below name an exact version, so a fixture
  // registry that gained a newer `@pnpm.e2e/bravo-dep` would make them pass
  // without testing anything. The lockfile records every group, so it is
  // where the version this graph resolves to can be checked.
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toContain(DEV_TRANSITIVE)

  for (const pkg of [PROD_DIRECT, SHARED]) {
    expect(storeHolds(pkg)).toBe(true)
  }
  for (const pkg of [DEV_DIRECT, DEV_TRANSITIVE]) {
    expect(storeHolds(pkg)).toBe(false)
  }
})

test('production install still moves aside a dependency another package manager installed', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/has-foo-100.1.0-dep-1': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/bravo': '1.0.0',
    },
  })

  // A real directory rather than a symlink, the way npm leaves one behind.
  fs.mkdirSync('node_modules/@pnpm.e2e/has-foo-100.1.0-dep-1', { recursive: true })
  fs.writeFileSync(
    'node_modules/@pnpm.e2e/has-foo-100.1.0-dep-1/package.json',
    JSON.stringify({ name: '@pnpm.e2e/has-foo-100.1.0-dep-1', version: '0.0.1-alien' })
  )

  const { stdout } = execPnpmSync(['install', '--prod'], {
    env: { pnpm_config_silent: 'false' },
    stdio: 'pipe',
    expectSuccess: true,
  })

  expect(stdout.toString()).toContain('was installed by a different package manager')
  expect(fs.existsSync('node_modules/.ignored/@pnpm.e2e/has-foo-100.1.0-dep-1')).toBe(true)
  expect(fs.lstatSync('node_modules/@pnpm.e2e/has-foo-100.1.0-dep-1').isSymbolicLink()).toBe(true)
})

test.each([
  ['--lockfile-only'],
  ['--dry-run'],
])('%s leaves a dependency another package manager installed untouched', async (flag) => {
  prepare({
    dependencies: {
      '@pnpm.e2e/has-foo-100.1.0-dep-1': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/bravo': '1.0.0',
    },
  })

  const alien = 'node_modules/@pnpm.e2e/has-foo-100.1.0-dep-1'
  fs.mkdirSync(alien, { recursive: true })
  fs.writeFileSync(
    `${alien}/package.json`,
    JSON.stringify({ name: '@pnpm.e2e/has-foo-100.1.0-dep-1', version: '0.0.1-alien' })
  )

  // No materialization pass follows, so nothing may move the entry aside even
  // though the group filter is active.
  execPnpmSync(['install', '--prod', flag], {
    env: { pnpm_config_silent: 'false' },
    stdio: 'pipe',
    expectSuccess: true,
  })

  expect(fs.existsSync('node_modules/.ignored')).toBe(false)
  expect(JSON.parse(fs.readFileSync(`${alien}/package.json`, 'utf8')).version).toBe('0.0.1-alien')
})
