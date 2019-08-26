import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare, { tempDir } from '@pnpm/prepare'
import { stripIndents } from 'common-tags'
import chalk from 'chalk'
import makeDir = require('make-dir')
import fs = require('mz/fs')
import normalizeNewline = require('normalize-newline')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync } from './utils'
import outdated from '../src/cmd/outdated'

const hasOutdatedDepsFixture = path.join(__dirname, 'packages', 'has-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = path.join(__dirname, 'packages', 'has-outdated-deps-and-external-shrinkwrap', 'pkg')
const hasNotOutdatedDepsFixture = path.join(__dirname, 'packages', 'has-not-outdated-deps')
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm outdated', async (t: tape.Test) => {
  process.chdir(hasOutdatedDepsFixture)

  t.equal(
    await outdated([], {
      alwaysAuth: false,
      fetchRetries: 2,
      fetchRetryFactor: 1,
      fetchRetryMaxtimeout: 60000,
      fetchRetryMintimeout: 10000,
      independentLeaves: false,
      networkConcurrency: 16,
      offline: false,
      prefix: process.cwd(),
      rawNpmConfig: {
        registry: 'https://localhost:4873',
      },
      strictSsl: false,
      tag: 'latest',
      userAgent: '',
      registries: { default: 'https://localhost:4873' },
      global: false,
    }, 'outdated'),
    stripIndents`
      ${chalk.underline('Package')}      ${chalk.underline('Current')}               ${chalk.underline('Latest')}
      is-positive  1.0.0 (wanted 3.1.0)  3.1.0
      is-negative  1.0.0 (wanted 1.1.0)  ${chalk.redBright.bold('2.1.0')}
    `,
  )
})

test('pnpm outdated: only current lockfile is available', async (t: tape.Test) => {
  tempDir(t)

  await makeDir(path.resolve('node_modules'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm-lock.yaml'), path.resolve('node_modules/.pnpm-lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current  Latest
    is-negative  1.0.0    2.1.0
    is-positive  1.0.0    3.1.0
  ` + '\n')
})

test('pnpm outdated: only wanted lockfile is available', async (t: tape.Test) => {
  tempDir(t)

  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const result = execPnpmSync('outdated')

  t.equal(result.status, 0)

  t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
    Package      Current                 Latest
    is-positive  missing (wanted 3.1.0)  3.1.0
    is-negative  missing (wanted 1.1.0)  2.1.0
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
    Package      Current               Latest
    is-positive  1.0.0 (wanted 3.1.0)  3.1.0
    is-negative  1.0.0 (wanted 1.1.0)  2.1.0
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
    Package      Current  Latest
    is-negative  1.0.0    2.1.0
    is-positive  1.0.0    3.1.0
  ` + '\n')
})

test(`pnpm outdated should fail when there is no ${WANTED_LOCKFILE} file in the root of the project`, async (t: tape.Test) => {
  prepare(t)

  const result = execPnpmSync('outdated')

  t.equal(result.status, 1)
  t.ok(result.stdout.toString().includes('No lockfile in this directory. Run `pnpm install` to generate one.'))
})
