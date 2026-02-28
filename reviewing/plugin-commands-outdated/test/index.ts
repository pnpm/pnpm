/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type PnpmError } from '@pnpm/error'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { prepare, tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { stripVTControlCharacters as stripAnsi } from 'util'
import symlinkDir from 'symlink-dir'

const f = fixtures(import.meta.dirname)
const hasOutdatedDepsFixture = f.find('has-outdated-deps')
const has2OutdatedDepsFixture = f.find('has-2-outdated-deps')
const hasOutdatedDepsFixtureAndExternalLockfile = path.join(f.find('has-outdated-deps-and-external-shrinkwrap'), 'pkg')
const hasNotOutdatedDepsFixture = f.find('has-not-outdated-deps')
const hasMajorOutdatedDepsFixture = f.find('has-major-outdated-deps')
const hasNoLockfileFixture = f.find('has-no-lockfile')
const withPnpmUpdateIgnore = f.find('with-pnpm-update-ignore')
const hasOutdatedDepsUsingCatalogProtocol = f.find('has-outdated-deps-using-catalog-protocol')
const hasOutdatedDepsUsingNpmAlias = f.find('has-outdated-deps-using-npm-alias')
const hasOnlyDeprecatedDepsFixture = f.find('has-only-deprecated-deps')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const OUTDATED_OPTIONS = {
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

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

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

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(has2OutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(has2OutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

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

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

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

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      format: 'list',
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
      format: 'list',
      long: true,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(`@pnpm.e2e/deprecated
1.0.0 => Deprecated
This package is deprecated. Lorem ipsum dolor sit amet, consectetur adipiscing elit.
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

test('pnpm outdated: format json', async () => {
  tempDir()

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  {
    const { output, exitCode } = await outdated.handler({
      ...OUTDATED_OPTIONS,
      dir: process.cwd(),
      format: 'json',
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toBe(JSON.stringify({
      '@pnpm.e2e/deprecated': {
        current: '1.0.0',
        latest: '1.0.0',
        wanted: '1.0.0',
        isDeprecated: true,
        dependencyType: 'dependencies',
      },
      'is-negative': {
        current: '1.0.0',
        latest: '2.1.0',
        wanted: '1.0.0',
        isDeprecated: false,
        dependencyType: 'dependencies',
      },
      'is-positive': {
        current: '1.0.0',
        latest: '3.1.0',
        wanted: '1.0.0',
        isDeprecated: false,
        dependencyType: 'devDependencies',
      },
    }, null, 2))
  }
})

test('pnpm outdated: format json when there are no outdated dependencies', async () => {
  prepare()

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    format: 'json',
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe('{}')
})

test('pnpm outdated: only current lockfile is available', async () => {
  tempDir()

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

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

  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

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

test('ignore packages in package.json > pnpm.updateConfig.ignoreDependencies in outdated command', async () => {
  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: withPnpmUpdateIgnore,
    updateConfig: {
      ignoreDependencies: [
        'is-positive',
      ],
    },
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌─────────────┬─────────┬────────┐
│ Package     │ Current │ Latest │
├─────────────┼─────────┼────────┤
│ is-negative │ 1.0.0   │ 2.1.0  │
└─────────────┴─────────┴────────┘
`)
})

test('pnpm outdated: catalog protocol', async () => {
  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    catalogs: {
      // Duplicating the catalog config in the pnpm-workspace.yaml inline to
      // avoid an async read and catalog config normalization call.
      default: { 'is-negative': '^1.0.0' },
    },
    dir: hasOutdatedDepsUsingCatalogProtocol,
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌─────────────┬─────────┬────────┐
│ Package     │ Current │ Latest │
├─────────────┼─────────┼────────┤
│ is-negative │ 1.0.0   │ 2.1.0  │
└─────────────┴─────────┴────────┘
`)
})

test('pnpm outdated: --compatible works with npm aliases', async () => {
  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    compatible: true,
    dir: hasOutdatedDepsUsingNpmAlias,
  })

  // Although is-negative@2.1.0 is the latest version at the time of writing,
  // the "compatible: true" option above should make pnpm to only find 1.0.1.
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌─────────────┬─────────┬────────┐
│ Package     │ Current │ Latest │
├─────────────┼─────────┼────────┤
│ is-negative │ 1.0.0   │ 1.0.1  │
└─────────────┴─────────┴────────┘
`)
})

test('pnpm outdated: support --sortField option', async () => {
  tempDir()

  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'pnpm-lock.yaml'), path.resolve('pnpm-lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: hasOutdatedDepsFixture,
    sortBy: 'name',
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬──────────────────────┬────────────┐
│ Package              │ Current              │ Latest     │
├──────────────────────┼──────────────────────┼────────────┤
│ @pnpm.e2e/deprecated │ 1.0.0                │ Deprecated │
├──────────────────────┼──────────────────────┼────────────┤
│ is-negative          │ 1.0.0 (wanted 2.1.0) │ 2.1.0      │
├──────────────────────┼──────────────────────┼────────────┤
│ is-positive (dev)    │ 1.0.0 (wanted 3.1.0) │ 3.1.0      │
└──────────────────────┴──────────────────────┴────────────┘
`)
})

test('pnpm outdated -g: shows outdated global packages', async () => {
  tempDir()

  // Set up a simulated global dir with one isolated package group
  const globalPkgDir = path.resolve('global')
  const installDir = path.join(globalPkgDir, 'install-1')
  fs.mkdirSync(path.join(installDir, 'node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.join(installDir, 'node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOutdatedDepsFixture, 'package.json'), path.join(installDir, 'package.json'))

  // Create symlink from a hash entry to the install dir (this is how scanGlobalPackages discovers packages)
  symlinkDir.sync(installDir, path.join(globalPkgDir, 'abc123'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: globalPkgDir,
    global: true,
    globalPkgDir,
    format: 'json',
  })

  expect(exitCode).toBe(1)
  const result = JSON.parse(stripAnsi(output))
  expect(result['is-negative']).toBeDefined()
  expect(result['@pnpm.e2e/deprecated']).toBeDefined()
})

test('pnpm outdated -g: no outdated packages when global dir is empty', async () => {
  tempDir()

  const globalPkgDir = path.resolve('global')
  fs.mkdirSync(globalPkgDir, { recursive: true })

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: globalPkgDir,
    global: true,
    globalPkgDir,
  })

  expect(output).toBe('')
  expect(exitCode).toBe(0)
})

test('pnpm outdated --long with only deprecated packages', async () => {
  tempDir()

  fs.mkdirSync(path.resolve('node_modules/.pnpm'), { recursive: true })
  fs.copyFileSync(path.join(hasOnlyDeprecatedDepsFixture, 'node_modules/.pnpm/lock.yaml'), path.resolve('node_modules/.pnpm/lock.yaml'))
  fs.copyFileSync(path.join(hasOnlyDeprecatedDepsFixture, 'package.json'), path.resolve('package.json'))

  const { output, exitCode } = await outdated.handler({
    ...OUTDATED_OPTIONS,
    dir: process.cwd(),
    long: true,
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`\
┌──────────────────────┬─────────┬────────────┬──────────────────────────────────────────┐
│ Package              │ Current │ Latest     │ Details                                  │
├──────────────────────┼─────────┼────────────┼──────────────────────────────────────────┤
│ @pnpm.e2e/deprecated │ 1.0.0   │ Deprecated │ This package is deprecated. Lorem ipsum  │
│                      │         │            │ dolor sit amet, consectetur adipiscing   │
│                      │         │            │ elit.                                    │
│                      │         │            │ https://foo.bar/qar                      │
└──────────────────────┴─────────┴────────────┴──────────────────────────────────────────┘
`)
})
