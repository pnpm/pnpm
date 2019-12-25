///<reference path="../../../typings/index.d.ts" />
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { outdated } from '@pnpm/plugin-commands-outdated'
import prepare, { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { stripIndent } from 'common-tags'
import fs = require('fs')
import makeDir = require('make-dir')
import path = require('path')
import stripAnsi = require('strip-ansi')
import test = require('tape')
import { promisify } from 'util'
import './recursive'

const copyFile = promisify(fs.copyFile)
const fixtures = path.join(__dirname, '../../../fixtures')
const hasOutdatedDepsFixture = path.join(fixtures, 'has-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = path.join(fixtures, 'has-outdated-deps-and-external-shrinkwrap', 'pkg')
const hasNotOutdatedDepsFixture = path.join(fixtures, 'has-not-outdated-deps')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const OUTDATED_OPTIONS = {
  alwaysAuth: false,
  fetchRetries: 1,
  fetchRetryFactor: 1,
  fetchRetryMaxtimeout: 60,
  fetchRetryMintimeout: 10,
  global: false,
  independentLeaves: false,
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

  await makeDir(path.resolve('node_modules/.pnpm'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const output = await outdated.handler([], {
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    long: true,
  }, 'outdated')

  t.equal(stripAnsi(output), stripIndent`
  ┌─────────────┬─────────┬────────────┬─────────────────────────────────────────────┐
  │ Package     │ Current │ Latest     │ Details                                     │
  ├─────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
  │ deprecated  │ 1.0.0   │ Deprecated │ This package is deprecated. Lorem ipsum     │
  │             │         │            │ dolor sit amet, consectetur adipiscing      │
  │             │         │            │ elit.                                       │
  │             │         │            │ https://foo.bar/qar                         │
  ├─────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
  │ is-negative │ 1.0.0   │ 2.1.0      │ https://github.com/kevva/is-negative#readme │
  ├─────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
  │ is-positive │ 1.0.0   │ 3.1.0      │ https://github.com/kevva/is-positive#readme │
  └─────────────┴─────────┴────────────┴─────────────────────────────────────────────┘
  ` + '\n')
  t.end()
})

test('pnpm outdated: no table', async (t) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules/.pnpm'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const output = await outdated.handler([], {
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      table: false,
    }, 'outdated')

    t.equal(stripAnsi(output), stripIndent`
    deprecated
    1.0.0 => Deprecated

    is-negative
    1.0.0 => 2.1.0

    is-positive
    1.0.0 => 3.1.0
    ` + '\n')
  }

  {
    const output = await outdated.handler([], {
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      long: true,
      table: false,
    }, 'outdated')

    t.equal(stripAnsi(output), stripIndent`
    deprecated
    1.0.0 => Deprecated
    This package is deprecated. Lorem ipsum
    dolor sit amet, consectetur adipiscing
    elit.
    https://foo.bar/qar

    is-negative
    1.0.0 => 2.1.0
    https://github.com/kevva/is-negative#readme

    is-positive
    1.0.0 => 3.1.0
    https://github.com/kevva/is-positive#readme
    ` + '\n')
  }
  t.end()
})

test('pnpm outdated: only current lockfile is available', async (t) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules/.pnpm'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const output = await outdated.handler([], {
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  }, 'outdated')

  t.equal(stripAnsi(output), stripIndent`
  ┌─────────────┬─────────┬────────────┐
  │ Package     │ Current │ Latest     │
  ├─────────────┼─────────┼────────────┤
  │ deprecated  │ 1.0.0   │ Deprecated │
  ├─────────────┼─────────┼────────────┤
  │ is-negative │ 1.0.0   │ 2.1.0      │
  ├─────────────┼─────────┼────────────┤
  │ is-positive │ 1.0.0   │ 3.1.0      │
  └─────────────┴─────────┴────────────┘
  ` + '\n')
  t.end()
})

test('pnpm outdated: only wanted lockfile is available', async (t) => {
  tempDir(t)

  await copyFile(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  await copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const output = await outdated.handler([], {
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  }, 'outdated')

  t.equal(stripAnsi(output), stripIndent`
  ┌─────────────┬────────────────────────┬────────────┐
  │ Package     │ Current                │ Latest     │
  ├─────────────┼────────────────────────┼────────────┤
  │ deprecated  │ missing (wanted 1.0.0) │ Deprecated │
  ├─────────────┼────────────────────────┼────────────┤
  │ is-positive │ missing (wanted 3.1.0) │ 3.1.0      │
  ├─────────────┼────────────────────────┼────────────┤
  │ is-negative │ missing (wanted 1.1.0) │ 2.1.0      │
  └─────────────┴────────────────────────┴────────────┘
  ` + '\n')
  t.end()
})

test('pnpm outdated does not print anything when all is good', async (t) => {
  process.chdir(hasNotOutdatedDepsFixture)

  const output = await outdated.handler([], {
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  }, 'outdated')

  t.equal(output, '')
  t.end()
})

test('pnpm outdated with external lockfile', async (t) => {
  process.chdir(hasOutdatedDepsFixtureAndExternalLockfile)

  const output = await outdated.handler([], {
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    lockfileDir: path.resolve('..'),
  }, 'outdated')

  t.equal(stripAnsi(output), stripIndent`
  ┌─────────────┬──────────────────────┬────────┐
  │ Package     │ Current              │ Latest │
  ├─────────────┼──────────────────────┼────────┤
  │ is-positive │ 1.0.0 (wanted 3.1.0) │ 3.1.0  │
  ├─────────────┼──────────────────────┼────────┤
  │ is-negative │ 1.0.0 (wanted 1.1.0) │ 2.1.0  │
  └─────────────┴──────────────────────┴────────┘
  ` + '\n')
  t.end()
})

test(`pnpm outdated should fail when there is no ${WANTED_LOCKFILE} file in the root of the project`, async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    const output = await outdated.handler([], {
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
    }, 'outdated')
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_OUTDATED_NO_LOCKFILE')
  t.end()
})
