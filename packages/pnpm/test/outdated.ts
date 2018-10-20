import prepare, { tempDir } from '@pnpm/prepare'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import { stripIndents } from 'common-tags'
import { execPnpmSync } from './utils'
import normalizeNewline = require('normalize-newline')

const hasOutdatedDepsFixture = path.join(__dirname, 'packages', 'has-outdated-deps')
const hasOutdatedDepsFixtureAndExternalShrinkwrap = path.join(__dirname, 'packages', 'has-outdated-deps-and-external-shrinkwrap', 'pkg')
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

test('pnpm outdated with external shrinkwrap', async (t: tape.Test) => {
  process.chdir(hasOutdatedDepsFixtureAndExternalShrinkwrap)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Wanted  Latest
    is-negative  1.0.0    1.1.0   2.1.0
    is-positive  1.0.0    3.1.0   3.1.0
  ` + '\n')
})
