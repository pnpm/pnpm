// cspell:ignore Ffoo -- `%2F` percent-encoding of `packages/foo` in the hoisting-limits locator keys
import { expect, test } from '@jest/globals'
import * as dp from '@pnpm/deps.path'
import { getHoistingLimits, hoist, type HoisterResult } from '@pnpm/installing.linking.real-hoist'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.utils'
import { fixtures } from '@pnpm/test-fixtures'
import type { DepPath, ProjectId } from '@pnpm/types'

const testFixtures = fixtures(import.meta.dirname)

test('hoist', async () => {
  const lockfile = await readWantedLockfile(testFixtures.find('fixture'), { ignoreIncompatible: true })
  expect(hoist(lockfile!)).toBeTruthy()
})

test.each([
  ['a package waiting on its peer provider', 'dependency-of-peer-dependent', 'package10', 'package02'],
  ['a package that nests the dependent', 'dependency-of-nested-package', 'package32', 'package20'],
])('hoist keeps %s below the dependency hoisted to its parent', async (_, fixture, name, parentName) => {
  const lockfile = await readWantedLockfile(testFixtures.find(fixture), { ignoreIncompatible: true })

  const result = hoist(lockfile!, { autoInstallPeers: false })

  expect(dependenciesResolvedToAnotherVersion(lockfile!, result)).toStrictEqual([])
  expect(parentsOf(result, name)).toContain(parentName)
})

test.each([
  'dependencies',
  'optionalDependencies',
] as const)('hoist keeps a package from %s that is also declared as a peer when peers are not auto-installed', (depField) => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        devDependencies: { parent: '1.0.0' },
        specifiers: { parent: '1.0.0' },
      },
    },
    packages: {
      ['parent@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-parent' },
        [depField]: { peer: '1.0.0' },
        peerDependencies: { peer: '*' },
      },
      ['peer@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-peer' },
        dependencies: { child: '1.0.0' },
      },
      ['child@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-child' },
      },
    },
  }

  const result = hoist(lockfile, { autoInstallPeers: false })
  const hoistedNames = new Set<string>()
  const pending = [result]
  while (pending.length > 0) {
    const current = pending.pop()!
    for (const dependency of current.dependencies) {
      hoistedNames.add(dependency.name)
      pending.push(dependency)
    }
  }

  expect(hoistedNames).toEqual(new Set(['parent', 'peer', 'child']))
})

test.each([
  ['unpatched', '17.0.0', '17.0.0'],
  ['patched, with deduplicated peers', '17.0.0(patch_hash=abc)', '17.0.0'],
])('hoist does not give a package its own copy of a peer that its parent provides (%s)', (_, react17, peerSuffixVersion) => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { app: '1.0.0', react: '18.0.0' },
        specifiers: { app: '1.0.0', react: '18.0.0' },
      },
    },
    packages: {
      ['app@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-app' },
        dependencies: { react: react17, widget: `1.0.0(react@${peerSuffixVersion})` },
      },
      [`widget@1.0.0(react@${peerSuffixVersion})` as DepPath]: {
        resolution: { integrity: 'sha512-widget' },
        dependencies: { react: react17 },
        peerDependencies: { react: '*' },
      },
      [`react@${react17}` as DepPath]: {
        resolution: { integrity: 'sha512-react17' },
      },
      ['react@18.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-react18' },
      },
    },
  }

  const result = hoist(lockfile, { autoInstallPeers: false })
  const app = [...result.dependencies].find(({ name }) => name === 'app')!
  const widget = [...app.dependencies].find(({ name }) => name === 'widget')

  expect([...result.dependencies].map(({ name }) => name).sort()).toEqual(['app', 'react'])
  expect([...app.dependencies].map(({ name }) => name).sort()).toEqual(['react', 'widget'])
  expect(widget?.dependencies.size).toBe(0)
})

test.each([
  ['Set', (name: string) => new Set([name])],
  ['Array', (name: string) => [name]],
  ['Generator', function * (name: string) {
    yield name
  }],
])('hoist strips externalDependencies passed as a %s from the root dependencies', async (_kind, toExternalDependencies) => {
  const lockfile = await readWantedLockfile(testFixtures.find('fixture'), { ignoreIncompatible: true })
  const withoutExternals = hoist(lockfile!)
  const [targetDep] = withoutExternals.dependencies
  expect(targetDep).toBeDefined()

  const withExternals = hoist(lockfile!, {
    externalDependencies: toExternalDependencies(targetDep.name),
  })
  expect([...withExternals.dependencies].some((dep) => dep.name === targetDep.name)).toBe(false)
})

test('hoist throws an error if the lockfile is broken', () => {
  expect(() => hoist({
    lockfileVersion: '5',
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
    packages: {},
  })).toThrow(/Broken lockfile/)
})

const importersLockfile: Pick<LockfileObject, 'importers'> = {
  importers: {
    ['.' as ProjectId]: {
      dependencies: { a: '1.0.0' },
      specifiers: { a: '1.0.0' },
    },
    ['packages/foo' as ProjectId]: {
      dependencies: { b: '1.0.0' },
      specifiers: { b: '1.0.0' },
    },
  },
}

test('getHoistingLimits returns undefined for the default "none" mode', () => {
  expect(getHoistingLimits(importersLockfile, undefined)).toBeUndefined()
  expect(getHoistingLimits(importersLockfile, 'none')).toBeUndefined()
})

test('getHoistingLimits in "workspaces" mode borders each workspace package at the root', () => {
  const limits = getHoistingLimits(importersLockfile, 'workspaces')
  // Only the root locator gets a border: its direct deps plus every
  // (URI-encoded) workspace package id. No per-importer border.
  expect([...limits!.keys()]).toStrictEqual(['.@'])
  expect(limits!.get('.@')).toStrictEqual(new Set(['a', 'packages%2Ffoo']))
})

test('getHoistingLimits in "dependencies" mode additionally borders each importer\'s direct deps', () => {
  const limits = getHoistingLimits(importersLockfile, 'dependencies')
  expect([...limits!.keys()].sort()).toStrictEqual(['.@', 'packages%2Ffoo@workspace:packages/foo'])
  expect(limits!.get('.@')).toStrictEqual(new Set(['a', 'packages%2Ffoo']))
  // Each non-root importer borders its own direct deps so their
  // transitives stay nested under them.
  expect(limits!.get('packages%2Ffoo@workspace:packages/foo')).toStrictEqual(new Set(['b']))
})

test('getHoistingLimits preserves synthetic root border when root importer is omitted', () => {
  const filteredLockfile: Pick<LockfileObject, 'importers'> = {
    importers: {
      ['packages/foo' as ProjectId]: {
        dependencies: { b: '1.0.0' },
        specifiers: { b: '1.0.0' },
      },
    },
  }
  const limits = getHoistingLimits(filteredLockfile, 'workspaces')
  expect([...limits!.keys()]).toStrictEqual(['.@'])
  expect(limits!.get('.@')).toStrictEqual(new Set(['packages%2Ffoo']))
})

test('hoist keeps in a nested project what the enclosing project shadows', () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: { specifiers: {} },
      ['a' as ProjectId]: {
        dependencies: { x: '1.0.0' },
        specifiers: { x: '1.0.0' },
      },
      ['a/b' as ProjectId]: {
        dependencies: { x: '2.0.0', y: '1.0.0' },
        specifiers: { x: '2.0.0', y: '1.0.0' },
      },
      ['c' as ProjectId]: {
        dependencies: { x: '2.0.0', y: '2.0.0' },
        specifiers: { x: '2.0.0', y: '2.0.0' },
      },
      ['d' as ProjectId]: {
        dependencies: { y: '2.0.0' },
        specifiers: { y: '2.0.0' },
      },
    },
    packages: {
      ['x@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-x1' } },
      ['x@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-x2' } },
      ['y@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-y1' }, dependencies: { x: '2.0.0' } },
      ['y@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-y2' } },
    },
  }

  const result = hoist(lockfile, { autoInstallPeers: false })

  expect(heldBy(result, 'a')).toStrictEqual(['x@1.0.0'])
  expect(heldBy(result, 'a%2Fb')).toStrictEqual(['x@2.0.0', 'y@1.0.0'])
})

test('hoist does not move the dependency of a nested project to the enclosing project', () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { z: '1.0.0' },
        specifiers: { z: '1.0.0' },
      },
      ['a' as ProjectId]: { specifiers: {} },
      ['a/b' as ProjectId]: {
        dependencies: { z: '2.0.0' },
        specifiers: { z: '2.0.0' },
      },
    },
    packages: {
      ['z@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-z1' } },
      ['z@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-z2' } },
    },
  }

  const result = hoist(lockfile, { autoInstallPeers: false })

  expect(heldBy(result, 'a')).toStrictEqual([])
  expect(heldBy(result, 'a%2Fb')).toStrictEqual(['z@2.0.0'])
})

/** What the root-level project `name` holds in its node_modules, by first reference. */
function heldBy (result: HoisterResult, name: string): string[] {
  const project = [...result.dependencies].find((dependency) => dependency.name === name)
  if (project == null) throw new Error(`No project ${name} at the root`)
  return [...project.dependencies].map((dependency) => [...dependency.references][0]).sort()
}

/**
 * Every dependency a hoisted package finds, walking up from its directory,
 * at another version than its snapshot records.
 */
function dependenciesResolvedToAnotherVersion (lockfile: LockfileObject, result: HoisterResult): string[] {
  return lookupPaths(result).flatMap((lookupPath) => mismatchesOf(lockfile, lookupPath))
}

/** Every hoisted directory, each followed by the directories above it, nearest first. */
function lookupPaths (result: HoisterResult): HoisterResult[][] {
  const paths: HoisterResult[][] = []
  const pending: HoisterResult[][] = [[result]]
  while (pending.length > 0) {
    const ancestors = pending.pop()!
    const children = [...ancestors[0].dependencies].map((directory) => [directory, ...ancestors])
    paths.push(...children)
    pending.push(...children)
  }
  return paths
}

function mismatchesOf (lockfile: LockfileObject, lookupPath: HoisterResult[]): string[] {
  const [directory] = lookupPath
  const snapshot = lockfile.packages?.[[...directory.references][0] as DepPath]
  return Object.entries({ ...snapshot?.dependencies, ...snapshot?.optionalDependencies })
    .filter(([alias]) => snapshot?.peerDependencies?.[alias] == null)
    .map(([alias, ref]) => ({ alias, expected: dp.refToRelative(ref, alias), found: resolvedPkgId(lookupPath, alias) }))
    .filter(({ expected, found }) => expected != null && dp.removeSuffix(expected) !== found)
    .map(({ alias, expected, found }) => `${directory.name} > ${alias}: expected ${expected}, found ${found}`)
}

function resolvedPkgId (lookupPath: HoisterResult[], alias: string): string | undefined {
  for (const level of lookupPath) {
    const found = [...level.dependencies].find((dependency) => dependency.name === alias)
    if (found != null) return dp.removeSuffix([...found.references][0])
  }
  return undefined
}

/** The names of the directories that hold a directory called `name`. */
function parentsOf (result: HoisterResult, name: string): string[] {
  const parents: string[] = []
  const pending = [result]
  while (pending.length > 0) {
    const current = pending.pop()!
    for (const dependency of current.dependencies) {
      if (dependency.name === name) parents.push(current.name)
      pending.push(dependency)
    }
  }
  return parents
}
