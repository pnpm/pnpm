/// <reference path="../../../typings/index.d.ts" />
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { outdated } from '@pnpm/plugin-commands-outdated'
import prepare, { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import './recursive'
import fs = require('mz/fs')
import path = require('path')
import stripAnsi = require('strip-ansi')
import test = require('tape')

const fixtures = path.join(__dirname, '../../../fixtures')
const hasOutdatedDepsFixture = path.join(fixtures, 'has-outdated-deps')
const has2OutdatedDepsFixture = path.join(fixtures, 'has-2-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = path.join(fixtures, 'has-outdated-deps-and-external-shrinkwrap', 'pkg')
const hasNotOutdatedDepsFixture = path.join(fixtures, 'has-not-outdated-deps')
const hasMajorOutdatedDepsFixture = path.join(fixtures, 'has-major-outdated-deps')
const hasNoLockfileFixture = path.join(fixtures, 'has-no-lockfile')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const OUTDATED_OPTIONS = {
  alwaysAuth: false,
  fetchRetries: 1,
  fetchRetryFactor: 1,
  fetchRetryMaxtimeout: 60,
  fetchRetryMintimeout: 10,
  global: false,
  networkConcurrency: 16,
  offline: false,
  rawConfig: { registry: REGISTRY_URL },
  registries: { default: REGISTRY_URL },
  strictSsl: false,
  tag: 'latest',
  userAgent: '',
}

test('pnpm outdated: show details', async (t) => {
  tempDir(t)

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    long: true,
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌───────────────────┬─────────┬────────────┬─────────────────────────────────────────────┐
│ Package           │ Current │ Latest     │ Details                                     │
├───────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ deprecated        │ 1.0.0   │ Deprecated │ This package is deprecated. Lorem ipsum     │
│                   │         │            │ dolor sit amet, consectetur adipiscing      │
│                   │         │            │ elit.                                       │
│                   │         │            │ https://foo.bar/qar                         │
├───────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ is-negative       │ 1.0.0   │ 2.1.0      │ https://github.com/kevva/is-negative#readme │
├───────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ is-positive (dev) │ 1.0.0   │ 3.1.0      │ https://github.com/kevva/is-positive#readme │
└───────────────────┴─────────┴────────────┴─────────────────────────────────────────────┘
`)
  t.end()
})

test('pnpm outdated: show details (using the public registry to verify that full metadata is being requested)', async (t) => {
  tempDir(t)

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(has2OutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(has2OutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    long: true,
    rawConfig: { registry: 'https://registry.npmjs.org/' },
    registries: { default: 'https://registry.npmjs.org/' },
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌───────────────────┬─────────┬────────┬─────────────────────────────────────────────┐
│ Package           │ Current │ Latest │ Details                                     │
├───────────────────┼─────────┼────────┼─────────────────────────────────────────────┤
│ is-negative       │ 1.0.1   │ 2.1.0  │ https://github.com/kevva/is-negative#readme │
├───────────────────┼─────────┼────────┼─────────────────────────────────────────────┤
│ is-positive (dev) │ 1.0.0   │ 3.1.0  │ https://github.com/kevva/is-positive#readme │
└───────────────────┴─────────┴────────┴─────────────────────────────────────────────┘
`)
  t.end()
})

test('pnpm outdated: showing only prod or dev dependencies', async (t) => {
  tempDir(t)

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      production: false,
    })

    t.equal(exitCode, 1)
    t.equal(stripAnsi(output), `\
┌───────────────────┬─────────┬────────┐
│ Package           │ Current │ Latest │
├───────────────────┼─────────┼────────┤
│ is-positive (dev) │ 1.0.0   │ 3.1.0  │
└───────────────────┴─────────┴────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dev: false,
      dir: process.cwd(),
    })

    t.equal(exitCode, 1)
    t.equal(stripAnsi(output), `\
┌─────────────┬─────────┬────────────┐
│ Package     │ Current │ Latest     │
├─────────────┼─────────┼────────────┤
│ deprecated  │ 1.0.0   │ Deprecated │
├─────────────┼─────────┼────────────┤
│ is-negative │ 1.0.0   │ 2.1.0      │
└─────────────┴─────────┴────────────┘
`)
  }

  t.end()
})

test('pnpm outdated: no table', async (t) => {
  tempDir(t)

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      table: false,
    })

    t.equal(exitCode, 1)
    t.equal(stripAnsi(output), `deprecated
1.0.0 => Deprecated

is-negative
1.0.0 => 2.1.0

is-positive (dev)
1.0.0 => 3.1.0
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      long: true,
      table: false,
    })

    t.equal(exitCode, 1)
    t.equal(stripAnsi(output), `deprecated
1.0.0 => Deprecated
This package is deprecated. Lorem ipsum
dolor sit amet, consectetur adipiscing
elit.
https://foo.bar/qar

is-negative
1.0.0 => 2.1.0
https://github.com/kevva/is-negative#readme

is-positive (dev)
1.0.0 => 3.1.0
https://github.com/kevva/is-positive#readme
`)
  }
  t.end()
})

test('pnpm outdated: only current lockfile is available', async (t) => {
  tempDir(t)

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌───────────────────┬─────────┬────────────┐
│ Package           │ Current │ Latest     │
├───────────────────┼─────────┼────────────┤
│ deprecated        │ 1.0.0   │ Deprecated │
├───────────────────┼─────────┼────────────┤
│ is-negative       │ 1.0.0   │ 2.1.0      │
├───────────────────┼─────────┼────────────┤
│ is-positive (dev) │ 1.0.0   │ 3.1.0      │
└───────────────────┴─────────┴────────────┘
`)
  t.end()
})

test('pnpm outdated: only wanted lockfile is available', async (t) => {
  tempDir(t)

  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌───────────────────┬────────────────────────┬────────────┐
│ Package           │ Current                │ Latest     │
├───────────────────┼────────────────────────┼────────────┤
│ deprecated        │ missing (wanted 1.0.0) │ Deprecated │
├───────────────────┼────────────────────────┼────────────┤
│ is-positive (dev) │ missing (wanted 3.1.0) │ 3.1.0      │
├───────────────────┼────────────────────────┼────────────┤
│ is-negative       │ missing (wanted 1.1.0) │ 2.1.0      │
└───────────────────┴────────────────────────┴────────────┘
`)
  t.end()
})

test('pnpm outdated does not print anything when all is good', async (t) => {
  process.chdir(hasNotOutdatedDepsFixture)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  t.equal(output, '')
  t.equal(exitCode, 0)
  t.end()
})

test('pnpm outdated with external lockfile', async (t) => {
  process.chdir(hasOutdatedDepsFixtureAndExternalLockfile)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    lockfileDir: path.resolve('..'),
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌─────────────┬──────────────────────┬────────┐
│ Package     │ Current              │ Latest │
├─────────────┼──────────────────────┼────────┤
│ is-positive │ 1.0.0 (wanted 3.1.0) │ 3.1.0  │
├─────────────┼──────────────────────┼────────┤
│ is-negative │ 1.0.0 (wanted 1.1.0) │ 2.1.0  │
└─────────────┴──────────────────────┴────────┘
`)
  t.end()
})

test(`pnpm outdated should fail when there is no ${WANTED_LOCKFILE} file in the root of the project`, async (t) => {
  process.chdir(hasNoLockfileFixture)

  let err!: PnpmError
  try {
    await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_OUTDATED_NO_LOCKFILE')
  t.end()
})

test('pnpm outdated should return empty when there is no lockfile and no dependencies', async (t) => {
  prepare(t)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  t.equal(output, '')
  t.equal(exitCode, 0)
  t.end()
})

test('pnpm outdated: print only compatible versions', async (t) => {
  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    compatible: true,
    dir: hasMajorOutdatedDepsFixture,
  })

  t.equal(exitCode, 1)
  t.equal(stripAnsi(output), `\
┌─────────────┬─────────┬────────┐
│ Package     │ Current │ Latest │
├─────────────┼─────────┼────────┤
│ is-negative │ 1.0.0   │ 1.0.1  │
└─────────────┴─────────┴────────┘
`)
  t.end()
})
