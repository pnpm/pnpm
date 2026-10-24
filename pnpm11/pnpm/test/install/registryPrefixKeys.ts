// One lockfile key per registry: a plain range routed by scope and a
// `<prefix>:` specifier that reach the same registry record one `packages:`
// entry, one snapshot, and one copy in `node_modules/.pnpm`.
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { ProjectManifest } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`
/** The same server under another host, which makes it another registry. */
const OTHER_REGISTRY = `http://127.0.0.1:${REGISTRY_MOCK_PORT}/npm/`
/** The routes to `REGISTRY` the workspace state records once `work` names it. */
const WORK_ROUTES = { 'route:@pnpm.e2e': 'work', 'route:default': 'work' }

/**
 * `@pnpm.e2e/foobar@100.0.0` declares `@pnpm.e2e/foo: ^100.0.0`, a plain range
 * the `@pnpm.e2e` scope routes.
 */
const PARENT = '@pnpm.e2e/foobar'
const CHILD = '@pnpm.e2e/foo'

function setup (dependencies: Record<string, string>, workspaceManifest: Record<string, unknown>): void {
  prepare({ name: 'test-registry-prefix-keys', version: '1.0.0', dependencies })
  writeYamlFileSync('pnpm-workspace.yaml', workspaceManifest)
}

function writeDependencies (dependencies: Record<string, string>): void {
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8')) as ProjectManifest
  fs.writeFileSync('package.json', JSON.stringify({ ...manifest, dependencies }))
}

function readLockfile (): LockfileFile {
  return readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
}

function writeLockfile (lockfile: LockfileFile): void {
  writeYamlFileSync(WANTED_LOCKFILE, lockfile)
}

function packageKeys (lockfile: LockfileFile, name: string): string[] {
  return keysOf(lockfile.packages, name)
}

function snapshotKeys (lockfile: LockfileFile, name: string): string[] {
  return keysOf(lockfile.snapshots, name)
}

function keysOf (entries: object | undefined, name: string): string[] {
  return Object.keys(entries ?? {}).filter((key) => key.startsWith(`${name}@`)).sort()
}

/** The `node_modules/.pnpm` directories holding a copy of `name`. */
function virtualStoreCopies (name: string): string[] {
  const prefix = `${name.replace('/', '+')}@`
  return fs.readdirSync('node_modules/.pnpm').filter((entry) => entry.startsWith(prefix)).sort()
}

function importerEntry (lockfile: LockfileFile, alias: string): unknown {
  return lockfile.importers?.['.']?.dependencies?.[alias]
}

function assertSingleChildCopy (expectedKey: string): void {
  const lockfile = readLockfile()
  expect(packageKeys(lockfile, CHILD)).toStrictEqual([expectedKey])
  expect(snapshotKeys(lockfile, CHILD)).toStrictEqual([expectedKey])
  expect(virtualStoreCopies(CHILD)).toHaveLength(1)
}

function readRecordedRegistryPrefixes (): unknown {
  const state = JSON.parse(fs.readFileSync('node_modules/.pnpm-workspace-state-v1.json', 'utf8'))
  return state.settings.registryPrefixes
}

// The prefix is spelled without the trailing slash the scope route and the
// default registry carry, which must not make it a different registry.
test('a plain scoped range and a prefixed range share one key with namedRegistries', async () => {
  setup({ [PARENT]: 'work:100.0.0', [CHILD]: 'work:^100.0.0' }, {
    namedRegistries: { work: REGISTRY.replace(/\/$/, '') },
    registries: { '@pnpm.e2e': REGISTRY },
  })

  await execPnpm(['install'])

  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')
  const lockfile = readLockfile()
  expect(packageKeys(lockfile, PARENT)).toStrictEqual(['@pnpm.e2e/foobar@work:100.0.0'])
  expect(importerEntry(lockfile, CHILD)).toStrictEqual({ specifier: 'work:^100.0.0', version: 'work:100.1.0' })

  await execPnpm(['install', '--frozen-lockfile'])
  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')
})

test('a plain scoped range and a prefixed range share one key with a registries declaration', async () => {
  setup({ [PARENT]: 'work:100.0.0', [CHILD]: 'work:^100.0.0' }, {
    registries: { [REGISTRY]: { scopes: ['@pnpm.e2e'], prefix: 'work' } },
  })

  await execPnpm(['install'])

  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')
  expect(packageKeys(readLockfile(), PARENT)).toStrictEqual(['@pnpm.e2e/foobar@work:100.0.0'])

  await execPnpm(['install', '--frozen-lockfile'])
  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')
})

test('the same name and version from two registries keeps separate keys', async () => {
  setup({ [CHILD]: 'work:100.1.0', 'foo-plain': 'npm:@pnpm.e2e/foo@100.1.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@pnpm.e2e': OTHER_REGISTRY },
  })

  await execPnpm(['install'])

  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual(['@pnpm.e2e/foo@100.1.0', '@pnpm.e2e/foo@work:100.1.0'])
  expect(virtualStoreCopies(CHILD)).toHaveLength(2)
})

test('several prefixes of one registry key its packages by the smallest one', async () => {
  setup({ [PARENT]: 'zeta:100.0.0', [CHILD]: 'work:^100.0.0' }, {
    namedRegistries: { zeta: REGISTRY, work: REGISTRY },
  })

  await execPnpm(['install'])

  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')
  const lockfile = readLockfile()
  expect(packageKeys(lockfile, PARENT)).toStrictEqual(['@pnpm.e2e/foobar@work:100.0.0'])
  expect(importerEntry(lockfile, PARENT)).toStrictEqual({ specifier: 'zeta:100.0.0', version: 'work:100.0.0' })
  const written = fs.readFileSync(WANTED_LOCKFILE, 'utf8')

  await execPnpm(['install', '--frozen-lockfile'])
  await execPnpm(['install'])
  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).toBe(written)
})

/**
 * Rewrites the lockfile to the shape written before plain ranges were keyed
 * by their registry: the child duplicated under a plain key, which the
 * parent's snapshot points at.
 */
function duplicateChildUnderPlainKey (version: string): void {
  const lockfile = readLockfile()
  const qualifiedKey = `${CHILD}@work:${version}`
  const plainKey = `${CHILD}@${version}`
  lockfile.packages![plainKey] = lockfile.packages![qualifiedKey]
  lockfile.snapshots![plainKey] = lockfile.snapshots![qualifiedKey]
  lockfile.snapshots![`${PARENT}@work:100.0.0`].dependencies![CHILD] = version
  writeLockfile(lockfile)
}

test('a frozen install keeps an older duplicate and a resolving install merges it', async () => {
  setup({ [PARENT]: 'work:100.0.0', [CHILD]: 'work:^100.0.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@pnpm.e2e': REGISTRY },
  })
  await execPnpm(['install'])
  expect(readRecordedRegistryPrefixes()).toStrictEqual({ work: REGISTRY, ...WORK_ROUTES })
  duplicateChildUnderPlainKey('100.1.0')
  fs.rmSync('node_modules', { recursive: true })

  await execPnpm(['install', '--frozen-lockfile'])

  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual(['@pnpm.e2e/foo@100.1.0', '@pnpm.e2e/foo@work:100.1.0'])
  expect(virtualStoreCopies(CHILD)).toHaveLength(2)
  // A frozen install cannot vouch for keys it did not write.
  expect(readRecordedRegistryPrefixes()).toBeUndefined()

  await execPnpm(['install'])

  const merged = readLockfile()
  expect(packageKeys(merged, CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.1.0'])
  expect(snapshotKeys(merged, CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.1.0'])
  expect(merged.snapshots?.[`${PARENT}@work:100.0.0`]?.dependencies?.[CHILD]).toBe('work:100.1.0')
  // Both edges link the one copy.
  const rootCopy = fs.realpathSync(path.join('node_modules', CHILD))
  const parentDir = fs.realpathSync(path.join('node_modules', PARENT))
  const nestedCopy = fs.realpathSync(path.join(path.dirname(parentDir), 'foo'))
  expect(rootCopy).toBe(nestedCopy)
  expect(rootCopy).toContain('work')
  expect(readRecordedRegistryPrefixes()).toStrictEqual({ work: REGISTRY, ...WORK_ROUTES })
})

/**
 * Rewrites the lockfile to the shape written before prefixed specifiers were
 * keyed by their registry: the root's child under a plain key, pinned at
 * `version`, with the root's specifier widened to a range.
 */
function widenRootChildToAPlainPin (version: string): void {
  const lockfile = readLockfile()
  lockfile.importers!['.'].dependencies![CHILD] = { specifier: 'work:^100.0.0', version }
  const qualifiedKey = `${CHILD}@work:${version}`
  const plainKey = `${CHILD}@${version}`
  lockfile.packages![plainKey] = lockfile.packages![qualifiedKey]
  delete lockfile.packages![qualifiedKey]
  lockfile.snapshots![plainKey] = lockfile.snapshots![qualifiedKey]
  delete lockfile.snapshots![qualifiedKey]
  writeLockfile(lockfile)
}

// The range admits 100.1.0, but the lockfile pins 100.0.0 and the resolving
// install only renames its key.
test('a resolving install re-keys without bumping the pinned version', async () => {
  setup({ [CHILD]: 'work:100.0.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@pnpm.e2e': REGISTRY },
  })
  await execPnpm(['install'])
  assertSingleChildCopy('@pnpm.e2e/foo@work:100.0.0')
  widenRootChildToAPlainPin('100.0.0')
  writeDependencies({ [CHILD]: 'work:^100.0.0' })

  await execPnpm(['install'])

  const merged = readLockfile()
  expect(packageKeys(merged, CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.0.0'])
  expect(importerEntry(merged, CHILD)).toStrictEqual({ specifier: 'work:^100.0.0', version: 'work:100.0.0' })
})

// The parent's `^100.0.0` stays on the recorded 100.0.0 although 100.1.0 is
// published.
test('a resolving install re-keys a transitive pin without bumping it', async () => {
  setup({ [PARENT]: 'work:100.0.0', [CHILD]: 'work:100.0.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@pnpm.e2e': REGISTRY },
  })
  await execPnpm(['install'])
  assertSingleChildCopy('@pnpm.e2e/foo@work:100.0.0')
  duplicateChildUnderPlainKey('100.0.0')

  await execPnpm(['install'])

  const merged = readLockfile()
  expect(packageKeys(merged, CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.0.0'])
  expect(snapshotKeys(merged, CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.0.0'])
})

// A filtered install keeps the keys of the importers it did not resolve, so it
// may leave an outdated key behind. The state vouches for the keys only when
// the lockfile on disk carries none.
test('the workspace state records the registry prefixes only for a lockfile whose keys carry them', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0', dependencies: { [CHILD]: '^100.0.0' } },
    { name: 'project-2', version: '1.0.0', dependencies: { [CHILD]: '^100.0.0' } },
  ])
  // `other` names a registry nothing here resolves from.
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['project-*'],
    registries: { '@pnpm.e2e': REGISTRY },
    namedRegistries: { other: OTHER_REGISTRY },
  })
  await execPnpm(['install'])
  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual(['@pnpm.e2e/foo@100.1.0'])
  expect(readRecordedRegistryPrefixes()).toStrictEqual({ other: OTHER_REGISTRY })

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['project-*'],
    registries: { '@pnpm.e2e': REGISTRY },
    namedRegistries: { other: OTHER_REGISTRY, work: REGISTRY },
  })
  await execPnpm(['install', '--filter', 'project-1'])

  const keysAreCurrent = packageKeys(readLockfile(), CHILD).every((key) => key.includes('@work:'))
  expect(readRecordedRegistryPrefixes()).toStrictEqual(keysAreCurrent ? { other: OTHER_REGISTRY, work: REGISTRY, ...WORK_ROUTES } : undefined)

  await execPnpm(['install'])

  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.1.0'])
  expect(readLockfile().importers?.['project-2']?.dependencies?.[CHILD]).toStrictEqual({ specifier: '^100.0.0', version: 'work:100.1.0' })
  expect(readRecordedRegistryPrefixes()).toStrictEqual({ other: OTHER_REGISTRY, work: REGISTRY, ...WORK_ROUTES })
})

test('the workspace state records the registry prefixes of an install that keeps no lockfile', async () => {
  setup({ [CHILD]: '^100.0.0' }, {
    lockfile: false,
    registries: { '@pnpm.e2e': REGISTRY },
    namedRegistries: { work: REGISTRY },
  })

  await execPnpm(['install'])

  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
  expect(readRecordedRegistryPrefixes()).toStrictEqual({ work: REGISTRY, ...WORK_ROUTES })
})

test('a frozen reinstall accepts a registry-qualified child of a local directory dependency', async () => {
  setup({ local: 'file:./local' }, {
    registries: { '@pnpm.e2e': REGISTRY },
    namedRegistries: { work: REGISTRY },
  })
  fs.mkdirSync('local')
  fs.writeFileSync('local/package.json', JSON.stringify({ name: 'local', version: '1.0.0', dependencies: { [CHILD]: '^100.0.0' } }))

  await execPnpm(['install'])
  expect(packageKeys(readLockfile(), CHILD)).toStrictEqual(['@pnpm.e2e/foo@work:100.1.0'])
  fs.rmSync('node_modules', { recursive: true })

  await execPnpm(['install', '--frozen-lockfile'])
  expect(virtualStoreCopies(CHILD)).toHaveLength(1)
})

test('jsr: specifiers share the key of the registry prefix', async () => {
  setup({ '@pnpm-e2e/bar': 'jsr:@pnpm-e2e/bar@2.0.0', 'bar-work': 'work:@jsr/pnpm-e2e__bar@2.0.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@jsr': REGISTRY },
  })

  await execPnpm(['install'])

  const lockfile = readLockfile()
  expect(packageKeys(lockfile, '@jsr/pnpm-e2e__bar')).toStrictEqual(['@jsr/pnpm-e2e__bar@work:2.0.0'])
  expect(snapshotKeys(lockfile, '@jsr/pnpm-e2e__bar')).toStrictEqual(['@jsr/pnpm-e2e__bar@work:2.0.0'])
  const written = fs.readFileSync(WANTED_LOCKFILE, 'utf8')

  fs.rmSync('node_modules', { recursive: true })
  await execPnpm(['install'])
  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).toBe(written)
})

test('the inspection commands read the merged keys', async () => {
  setup({ [PARENT]: 'work:100.0.0', [CHILD]: 'work:^100.0.0', 'foo-plain': 'npm:@pnpm.e2e/foo@^100.0.0' }, {
    namedRegistries: { work: REGISTRY },
    registries: { '@pnpm.e2e': REGISTRY },
  })
  await execPnpm(['install'])
  assertSingleChildCopy('@pnpm.e2e/foo@work:100.1.0')

  execPnpmSync(['dedupe', '--check'], { expectSuccess: true })
  execPnpmSync(['outdated'], { expectSuccess: true })

  const licenses = execPnpmSync(['licenses', 'list', '--json'], { expectSuccess: true })
  const report = JSON.parse(licenses.stdout.toString()) as Record<string, Array<{ name: string, versions: string[], registryName?: string }>>
  const fooRecords = Object.values(report).flat().filter((pkg) => pkg.name === CHILD)
  expect(fooRecords).toHaveLength(1)
  // A license record names the registry its lockfile key does.
  expect(fooRecords[0]).toMatchObject({ versions: ['100.1.0'], registryName: 'work' })

  const sbom = execPnpmSync(['sbom', '--sbom-format', 'cyclonedx'], { expectSuccess: true })
  const components = (JSON.parse(sbom.stdout.toString()) as { components: Array<{ name: string, group?: string }> }).components
  expect(components.filter((component) => component.name === 'foo' && component.group === '@pnpm.e2e')).toHaveLength(1)
})
