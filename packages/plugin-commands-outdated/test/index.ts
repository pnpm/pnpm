/// <reference path="../../../typings/index.d.ts" />
import { promises as fs } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { outdated } from '@pnpm/plugin-commands-outdated'
import prepare, { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import stripAnsi from 'strip-ansi'

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
  cacheDir: 'cache',
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
  userConfig: {},
}

test('pnpm outdated: show details', async () => {
  tempDir()

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    long: true,
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬─────────┬────────────┬─────────────────────────────────────────────┐
│ Package              │ Current │ Latest     │ Details                                     │
├──────────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ @pnpm.e2e/deprecated │ 1.0.0   │ Deprecated │ This package is deprecated. Lorem ipsum     │
│                      │         │            │ dolor sit amet, consectetur adipiscing      │
│                      │         │            │ elit.                                       │
│                      │         │            │ https://foo.bar/qar                         │
├──────────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ is-negative          │ 1.0.0   │ 2.1.0      │ https://github.com/kevva/is-negative#readme │
├──────────────────────┼─────────┼────────────┼─────────────────────────────────────────────┤
│ is-positive (dev)    │ 1.0.0   │ 3.1.0      │ https://github.com/kevva/is-positive#readme │
└──────────────────────┴─────────┴────────────┴─────────────────────────────────────────────┘
`)
})

test('pnpm outdated: show details (using the public registry to verify that full metadata is being requested)', async () => {
  tempDir()

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

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌───────────────────┬─────────┬────────┬─────────────────────────────────────────────┐
│ Package           │ Current │ Latest │ Details                                     │
├───────────────────┼─────────┼────────┼─────────────────────────────────────────────┤
│ is-negative       │ 1.0.1   │ 2.1.0  │ https://github.com/kevva/is-negative#readme │
├───────────────────┼─────────┼────────┼─────────────────────────────────────────────┤
│ is-positive (dev) │ 1.0.0   │ 3.1.0  │ https://github.com/kevva/is-positive#readme │
└───────────────────┴─────────┴────────┴─────────────────────────────────────────────┘
`)
})

test('pnpm outdated: showing only prod or dev dependencies', async () => {
  tempDir()

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      production: false,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(`\
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

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬─────────┬────────────┐
│ Package              │ Current │ Latest     │
├──────────────────────┼─────────┼────────────┤
│ @pnpm.e2e/deprecated │ 1.0.0   │ Deprecated │
├──────────────────────┼─────────┼────────────┤
│ is-negative          │ 1.0.0   │ 2.1.0      │
└──────────────────────┴─────────┴────────────┘
`)
  }
})

test('pnpm outdated: no table', async () => {
  tempDir()

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      table: false,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(`@pnpm.e2e/deprecated
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

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(`@pnpm.e2e/deprecated
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
})

test('pnpm outdated: only current lockfile is available', async () => {
  tempDir()

  await fs.mkdir(path.resolve('node_modules/.pnpm'), { recursive: true })
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬─────────┬────────────┐
│ Package              │ Current │ Latest     │
├──────────────────────┼─────────┼────────────┤
│ @pnpm.e2e/deprecated │ 1.0.0   │ Deprecated │
├──────────────────────┼─────────┼────────────┤
│ is-negative          │ 1.0.0   │ 2.1.0      │
├──────────────────────┼─────────┼────────────┤
│ is-positive (dev)    │ 1.0.0   │ 3.1.0      │
└──────────────────────┴─────────┴────────────┘
`)
})

test('pnpm outdated: only wanted lockfile is available', async () => {
  tempDir()

  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  await fs.copyFile(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬────────────────────────┬────────────┐
│ Package              │ Current                │ Latest     │
├──────────────────────┼────────────────────────┼────────────┤
│ @pnpm.e2e/deprecated │ missing (wanted 1.0.0) │ Deprecated │
├──────────────────────┼────────────────────────┼────────────┤
│ is-negative          │ missing (wanted 2.1.0) │ 2.1.0      │
├──────────────────────┼────────────────────────┼────────────┤
│ is-positive (dev)    │ missing (wanted 3.1.0) │ 3.1.0      │
└──────────────────────┴────────────────────────┴────────────┘
`)
})

test('pnpm outdated does not print anything when all is good', async () => {
  process.chdir(hasNotOutdatedDepsFixture)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  expect(output).toBe('')
  expect(exitCode).toBe(0)
})

test('pnpm outdated with external lockfile', async () => {
  process.chdir(hasOutdatedDepsFixtureAndExternalLockfile)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    lockfileDir: path.resolve('..'),
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌─────────────┬──────────────────────┬────────┐
│ Package     │ Current              │ Latest │
├─────────────┼──────────────────────┼────────┤
│ is-positive │ 1.0.0 (wanted 3.1.0) │ 3.1.0  │
├─────────────┼──────────────────────┼────────┤
│ is-negative │ 1.0.0 (wanted 1.1.0) │ 2.1.0  │
└─────────────┴──────────────────────┴────────┘
`)
})

test(`pnpm outdated should fail when there is no ${WANTED_LOCKFILE} file in the root of the project`, async () => {
  process.chdir(hasNoLockfileFixture)

  let err!: PnpmError
  try {
    await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_OUTDATED_NO_LOCKFILE')
})

test('pnpm outdated should return empty when there is no lockfile and no dependencies', async () => {
  prepare(undefined)

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
  })

  expect(output).toBe('')
  expect(exitCode).toBe(0)
})

test('pnpm outdated: print only compatible versions', async () => {
  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    compatible: true,
    dir: hasMajorOutdatedDepsFixture,
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌─────────────┬─────────┬────────┐
│ Package     │ Current │ Latest │
├─────────────┼─────────┼────────┤
│ is-negative │ 1.0.0   │ 1.0.1  │
└─────────────┴─────────┴────────┘
`)
})
