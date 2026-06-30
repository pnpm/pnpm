import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

// `pacquet` is fetched from the real npm registry — registry-mock doesn't
// carry it (or its platform-specific binary sub-packages). Tests are gated
// on the public registry being reachable.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'
// pacquet >= 0.11.7 supports full resolving installs, so pnpm delegates
// non-frozen plain installs to it too.
const PACQUET_VERSION = '0.11.7'
// pacquet < 0.11.7 stays on pnpm's resolve-then-materialize path.
const PACQUET_RESOLVE_WITH_PNPM_VERSION = '0.11.6'

// Each test runs two or three installs against the public registry; raise
// the per-test timeout above jest's 5s default to allow for cold caches.
const TIMEOUT = 5 * 60 * 1000

interface PrepareOpts {
  manifest?: { dependencies?: Record<string, string>, devDependencies?: Record<string, string> }
  /** Which `configDependencies` slot declares pacquet. Both work. */
  pacquetConfigDepName?: 'pacquet' | '@pnpm/pacquet'
  /** Which pacquet version to declare. Defaults to {@link PACQUET_VERSION}. */
  version?: string
}

/** Set up a temp project + workspace yaml + initial install. */
async function prepareWithPacquet (opts: PrepareOpts = {}): Promise<void> {
  prepare(opts.manifest ?? {})
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      [opts.pacquetConfigDepName ?? 'pacquet']: opts.version ?? PACQUET_VERSION,
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

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Using pacquet for this install')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('bare `pnpm install` (no --frozen-lockfile) delegates to pacquet when the lockfile is up to date', async () => {
  await prepareWithPacquet({ manifest: { dependencies: { 'is-positive': '3.1.0' } } })
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  // No `--frozen-lockfile` flag, but the lockfile is already up to date
  // with the manifest, so no resolution is needed: pnpm delegates the
  // whole install to pacquet just as it would for a frozen install.
  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Using pacquet for this install')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('pnpm install resolves a newly-added dependency with pacquet >= 0.11.7', async () => {
  // `prepare` installs with no dependencies, so the lockfile has no entry
  // for `is-positive`. Adding it to the manifest forces a real resolution
  // on the next install — which pacquet performs itself, in a single
  // non-frozen pass (resolve + materialize), without a pnpm resolve pass.
  await prepareWithPacquet()
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  manifest.dependencies = { 'is-positive': '3.1.0' }
  fs.writeFileSync('package.json', JSON.stringify(manifest, null, 2))

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  const output = stdout.toString()
  expect(output).toContain('Using pacquet for this install')
  expect(output).toContain('Progress: resolved')
  expect(output.indexOf('Using pacquet for this install')).toBeLessThan(output.indexOf('Progress: resolved'))
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('pnpm install resolves a newly-added dependency itself when pacquet < 0.11.7', async () => {
  // Same setup as the resolving test above, but with an older
  // pacquet: pnpm runs its own lockfileOnly resolve pass for the new dep
  // and hands the freshly-written lockfile to pacquet to materialize.
  await prepareWithPacquet({ version: PACQUET_RESOLVE_WITH_PNPM_VERSION })
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  manifest.dependencies = { 'is-positive': '3.1.0' }
  fs.writeFileSync('package.json', JSON.stringify(manifest, null, 2))

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  const output = stdout.toString()
  expect(output).toContain('Using pacquet for this install')
  expect(output).toContain('Progress: resolved')
  expect(output.indexOf('Progress: resolved')).toBeLessThan(output.indexOf('Using pacquet for this install'))
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)

test('`pnpm add <pkg>` resolves the new dep with pnpm and materializes with pacquet', async () => {
  await prepareWithPacquet()

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'add', 'is-positive@3.1.0'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  // Pnpm's resolver handles the new package; pacquet performs the
  // fetch / import. The delegation log fires on the materialization
  // pass that follows the resolve.
  expect(stdout.toString()).toContain('Using pacquet for this install')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
  // Package.json must record the new dep so subsequent installs see it.
  const manifest = JSON.parse(await fs.promises.readFile('package.json', 'utf8'))
  expect(manifest.dependencies?.['is-positive']).toBeDefined()
}, TIMEOUT)

test('`pnpm update <pkg>` resolves a new version with pnpm and materializes with pacquet', async () => {
  // Start pinned to an old exact version so `update --latest` has
  // something to do (is-positive's latest is 3.1.0).
  await prepareWithPacquet({ manifest: { dependencies: { 'is-positive': '1.0.0' } } })
  const oldVersion = JSON.parse(
    await fs.promises.readFile('node_modules/is-positive/package.json', 'utf8')
  ).version as string
  expect(oldVersion).toBe('1.0.0')

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'update', 'is-positive', '--latest'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Using pacquet for this install')
  const newVersion = JSON.parse(
    await fs.promises.readFile('node_modules/is-positive/package.json', 'utf8')
  ).version as string
  // `update --latest` moves the `1.0.0` pin to the current latest (3.1.0).
  expect(newVersion).not.toBe(oldVersion)
}, TIMEOUT)

test('the `@pnpm/pacquet` scoped alias is recognized in configDependencies', async () => {
  await prepareWithPacquet({
    manifest: { dependencies: { 'is-positive': '3.1.0' } },
    pacquetConfigDepName: '@pnpm/pacquet',
  })
  await fs.promises.rm('node_modules', { recursive: true, force: true })

  const { stdout, status } = execPnpmSync(
    [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
    { env: { pnpm_config_silent: 'false' }, stdio: 'pipe', expectSuccess: true }
  )
  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Using pacquet for this install')
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
}, TIMEOUT)
