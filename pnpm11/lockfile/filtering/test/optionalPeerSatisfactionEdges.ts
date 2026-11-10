import { describe, expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { filterLockfileByImporters, filterLockfileByImportersAndEngine } from '@pnpm/lockfile.filtering'
import { getPeerSatisfactionEdges } from '@pnpm/lockfile.peer-edges'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DependenciesField, DepPath, ProjectId } from '@pnpm/types'

type Include = { [dependenciesField in DependenciesField]: boolean }

const ALL: Include = { dependencies: true, devDependencies: true, optionalDependencies: true }
const PROD_ONLY: Include = { dependencies: true, devDependencies: false, optionalDependencies: true }

const ABC = 'abc@1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' as DepPath

function optionalPeerLockfile (): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
        devDependencies: { 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
        specifiers: { abc: '1.0.0', 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
      },
    },
    packages: {
      [ABC]: {
        resolution: { integrity: 'abc' },
        peerDependencies: { 'peer-a': '^1.0.0', 'peer-c': '^1.0.0' },
        peerDependenciesMeta: { 'peer-c': { optional: true } },
        dependencies: { 'peer-a': '1.0.0' },
        optionalDependencies: { 'peer-c': '1.0.0' },
      },
      ['peer-a@1.0.0' as DepPath]: { resolution: { integrity: 'peer-a' } },
      ['peer-c@1.0.0' as DepPath]: { resolution: { integrity: 'peer-c' } },
    },
  }
}

function filterByImporters (lockfile: LockfileObject, include: Include): LockfileObject {
  return filterLockfileByImporters(lockfile, Object.keys(lockfile.importers) as ProjectId[], {
    failOnMissingDependencies: true,
    include,
    skipped: new Set(),
  })
}

function filterByImportersAndEngine (lockfile: LockfileObject, include: Include): LockfileObject {
  return filterLockfileByImportersAndEngine(lockfile, Object.keys(lockfile.importers) as ProjectId[], {
    currentEngine: { pnpmVersion: '11.0.0' },
    engineStrict: false,
    failOnMissingDependencies: true,
    include,
    includeIncompatiblePackages: true,
    lockfileDir: process.cwd(),
    skipped: new Set(),
  }).lockfile
}

describe.each([
  ['filterLockfileByImporters', filterByImporters],
  ['filterLockfileByImportersAndEngine', filterByImportersAndEngine],
])('%s()', (_, filter) => {
  test('leaves out a package that only satisfies an optional peer through an excluded dependency group', () => {
    const lockfile = optionalPeerLockfile()
    const inputAbc = lockfile.packages![ABC]

    const filtered = filter(lockfile, PROD_ONLY)

    expect(Object.keys(filtered.packages!).sort()).toStrictEqual([ABC, 'peer-a@1.0.0'])
    expect(filtered.packages![ABC].dependencies).toStrictEqual({ 'peer-a': '1.0.0' })
    expect(filtered.packages![ABC].optionalDependencies).toBeUndefined()
    expect(lockfile.packages![ABC]).toBe(inputAbc)
    expect(inputAbc.optionalDependencies).toStrictEqual({ 'peer-c': '1.0.0' })
  })

  test('keeps the edge of a package that another path retains', () => {
    const lockfile = optionalPeerLockfile()
    lockfile.importers['.' as ProjectId].dependencies!['peer-c'] = '1.0.0'
    delete lockfile.importers['.' as ProjectId].devDependencies!['peer-c']

    const filtered = filter(lockfile, PROD_ONLY)

    expect(filtered.packages![ABC]).toBe(lockfile.packages![ABC])
  })

  test('changes nothing when every dependency group is included', () => {
    const lockfile = optionalPeerLockfile()

    const filtered = filter(lockfile, ALL)

    expect(filtered.packages).toStrictEqual(lockfile.packages)
    expect(filtered.packages![ABC]).toBe(lockfile.packages![ABC])
  })

  test('keeps every snapshot of a large lockfile unchanged when every dependency group is included', () => {
    const lockfile = createLargeOptionalPeerLockfile(500)
    expect(getPeerSatisfactionEdges(lockfile, { resolvePeersFromWorkspaceRoot: true }).size).toBeGreaterThan(0)

    const filtered = filter(lockfile, ALL)

    expect(Object.keys(filtered.packages!).sort()).toStrictEqual(Object.keys(lockfile.packages!).sort())
    for (const [depPath, snapshot] of Object.entries(filtered.packages!)) {
      expect(snapshot).toBe(lockfile.packages![depPath as DepPath])
    }
  })
})

function createLargeOptionalPeerLockfile (size: number): LockfileObject {
  const lockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: { dependencies: {}, devDependencies: {}, specifiers: {} },
    },
    packages: {},
  }
  const importer = lockfile.importers['.' as ProjectId]
  for (let index = 0; index < size; index++) {
    const name = `pkg-${index}`
    const peer = `peer-${index}`
    const version = `1.0.0(${peer}@1.0.0)`
    importer.dependencies![name] = version
    importer.devDependencies![peer] = '1.0.0'
    importer.specifiers[name] = '1.0.0'
    importer.specifiers[peer] = '1.0.0'
    lockfile.packages![`${name}@${version}` as DepPath] = {
      resolution: { integrity: name },
      peerDependencies: { [peer]: '^1.0.0' },
      peerDependenciesMeta: { [peer]: { optional: true } },
      optionalDependencies: { [peer]: '1.0.0' },
      ...(index > 0 && { dependencies: { [`pkg-${index - 1}`]: `1.0.0(peer-${index - 1}@1.0.0)` } }),
    }
    lockfile.packages![`${peer}@1.0.0` as DepPath] = { resolution: { integrity: peer } }
  }
  return lockfile
}
