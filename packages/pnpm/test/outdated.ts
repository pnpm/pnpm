import prepare, { tempDir } from '@pnpm/prepare'
import { stripIndents } from 'common-tags'
import normalizeNewline = require('normalize-newline')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync } from './utils'

const hasOutdatedDepsFixture = path.join(__dirname, 'packages', 'has-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = path.join(__dirname, 'packages', 'has-outdated-deps-and-external-shrinkwrap', 'pkg')
const hasNotOutdatedDepsFixture = path.join(__dirname, 'packages', 'has-not-outdated-deps')
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm outdated', async (t: tape.Test) => {
  process.chdir(hasOutdatedDepsFixture)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Wanted  Latest
    is-negative  1.0.0    1.1.0   2.1.0
    is-positive  1.0.0    3.1.0   3.1.0
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

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Wanted  Latest
    is-negative  1.0.0    1.1.0   2.1.0
    is-positive  1.0.0    3.1.0   3.1.0
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

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Wanted  Latest
    is-negative  1.0.0    1.0.0   2.1.0
    is-positive  1.0.0    1.0.0   3.1.0
  ` + '\n')
})
