import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare, { tempDir } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import makeDir = require('make-dir')
import fs = require('mz/fs')
import normalizeNewline = require('normalize-newline')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync, pathToLocalPkg } from './utils'

const hasOutdatedDepsFixture = pathToLocalPkg('has-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = pathToLocalPkg('has-outdated-deps-and-external-shrinkwrap/pkg')
const hasNotOutdatedDepsFixture = pathToLocalPkg('has-not-outdated-deps')
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm outdated: show details', async (t: tape.Test) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules/.pnpm'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const result = execPnpmSync('outdated', '--long')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
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
})

test('pnpm outdated: no table', async (t: tape.Test) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules/.pnpm'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const result = execPnpmSync('outdated', '--no-table')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    deprecated
    1.0.0 => Deprecated

    is-negative
    1.0.0 => 2.1.0

    is-positive
    1.0.0 => 3.1.0
    ` + '\n')
  }

  {
    const result = execPnpmSync('outdated', '--no-table', '--long')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
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
})

test('pnpm outdated: only current lockfile is available', async (t: tape.Test) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules/.pnpm'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
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
})

test('pnpm outdated: only wanted lockfile is available', async (t: tape.Test) => {
  tempDir(t)

  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
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
})

test('pnpm outdated does not print anything when all is good', async (t: tape.Test) => {
  process.chdir(hasNotOutdatedDepsFixture)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), '')
})

test('pnpm outdated with external lockfile', async (t: tape.Test) => {
  process.chdir(hasOutdatedDepsFixtureAndExternalLockfile)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
  ┌─────────────┬──────────────────────┬────────┐
  │ Package     │ Current              │ Latest │
  ├─────────────┼──────────────────────┼────────┤
  │ is-positive │ 1.0.0 (wanted 3.1.0) │ 3.1.0  │
  ├─────────────┼──────────────────────┼────────┤
  │ is-negative │ 1.0.0 (wanted 1.1.0) │ 2.1.0  │
  └─────────────┴──────────────────────┴────────┘
  ` + '\n')
})

test('pnpm outdated on global packages', async (t: tape.Test) => {
  prepare(t)
  const global = path.resolve('..', 'global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', 'is-negative@1.0.0', 'is-positive@1.0.0')

  const result = execPnpmSync('outdated', '-g')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
  ┌─────────────┬─────────┬────────┐
  │ Package     │ Current │ Latest │
  ├─────────────┼─────────┼────────┤
  │ is-negative │ 1.0.0   │ 2.1.0  │
  ├─────────────┼─────────┼────────┤
  │ is-positive │ 1.0.0   │ 3.1.0  │
  └─────────────┴─────────┴────────┘
  ` + '\n')
})

test(`pnpm outdated should fail when there is no ${WANTED_LOCKFILE} file in the root of the project`, async (t: tape.Test) => {
  prepare(t)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 1)
  t.ok(result.stdout.toString().includes('No lockfile in this directory. Run `pnpm install` to generate one.'))
})
