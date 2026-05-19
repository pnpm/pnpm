import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

// `pacquet` is fetched from the real npm registry — registry-mock doesn't
// carry it (or its platform-specific binary sub-packages). Pinned to a
// version known to ship the `configDependencies` integration surface this
// PR depends on; tests are gated on the public registry being reachable.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'
const PACQUET_VERSION = '0.2.2-9'

// Each test runs two or three installs against the public registry; raise
// the per-test timeout above jest's 5s default to allow for cold caches.
const TIMEOUT = 5 * 60 * 1000

interface PrepareOpts {
  manifest?: { dependencies?: Record<string, string>, devDependencies?: Record<string, string> }
  /** Which `configDependencies` slot declares pacquet. Both work. */
  pacquetConfigDepName?: 'pacquet' | '@pnpm/pacquet'
}

/** Set up a temp project + workspace yaml + initial install. */
async function prepareWithPacquet (opts: PrepareOpts = {}): Promise<void> {
  prepare(opts.manifest ?? {})
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      [opts.pacquetConfigDepName ?? 'pacquet']: PACQUET_VERSION,
    },
  })
  // Initial install populates pnpm-lock.yaml plus configDependencies
  // (pacquet + platform binary). This first install goes through the JS
  // path because `node_modules/.pnpm-config/pacquet` isn't on disk yet
  // for the delegate to use.
  await execPnpm([PUBLIC_REGISTRY, 'install'])
}

test('pnpm install --frozen-lockfile delegates to pacquet when declared in configDependencies', async () => {
  await prepareWithPacquet({ manifest: { dependencies: { 'is-positive': '3.1.0' } } })
  expect(fs.existsSync('node_modules/.pnpm-config/pacquet/bin/pacquet')).toBe(true)
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)

  // Wipe `node_modules` while leaving lockfiles intact — the CI-style
  // starting state of a checked-out repo with no installed modules.
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('bare `pnpm install` (no --frozen-lockfile) delegates the materialization to pacquet', async () => {
  await prepareWithPacquet({ manifest: { dependencies: { 'is-positive': '3.1.0' } } })
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  // No `--frozen-lockfile` flag. The expected path is: pnpm runs a
  // lockfileOnly resolve pass (the lockfile is already up-to-date so
  // it's a no-op write), then hands fetch / import / link off to
  // pacquet via the default-isolated-linker branch.
  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('`pnpm add <pkg>` resolves the new dep with pnpm and materializes with pacquet', async () => {
  await prepareWithPacquet()

  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'add', 'is-positive@3.1.0'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  // Pnpm's resolver handles the new package; pacquet performs the
  // fetch / import. The delegation log fires on the materialization
  // pass that follows the resolve.
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
  // Package.json must record the new dep so subsequent installs see it.
  const manifest = JSON.parse(await fs.promises.readFile('package.json', 'utf8'))
  expect(manifest.dependencies?.['is-positive']).toBeDefined()
}, TIMEOUT)

test('`pnpm update <pkg>` resolves a new version with pnpm and materializes with pacquet', async () => {
  // Start pinned to an older minor so `update` has something to do.
  await prepareWithPacquet({ manifest: { dependencies: { 'is-positive': '^3.0.0' } } })
  const oldVersion = JSON.parse(
    await fs.promises.readFile('node_modules/is-positive/package.json', 'utf8')
  ).version as string

  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'update', 'is-positive', '--latest'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  const newVersion = JSON.parse(
    await fs.promises.readFile('node_modules/is-positive/package.json', 'utf8')
  ).version as string
  // is-positive@4 is the current latest and is a major bump from the 3.x
  // line; `update --latest` should move past the original `^3.0.0` pin.
  expect(newVersion).not.toBe(oldVersion)
}, TIMEOUT)

// Skipped until pacquet ships a release built with the updated
// `generate-packages.mjs` (this PR's change) so the `@pnpm/pacquet`
// scoped alias actually exists on npm. The pinned `0.2.2-9` doesn't
// publish that mirror yet. Re-enable when the next pacquet release
// ships under both names.
test.skip('the `@pnpm/pacquet` scoped alias is recognized in configDependencies', async () => {
  await prepareWithPacquet({
    manifest: { dependencies: { 'is-positive': '3.1.0' } },
    pacquetConfigDepName: '@pnpm/pacquet',
  })
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  const { stderr, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stderr.toString()).toContain('Delegating install to pacquet')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)
