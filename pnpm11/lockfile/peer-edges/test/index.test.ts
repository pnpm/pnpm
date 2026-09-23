import { describe, expect, test } from '@jest/globals'
import {
  getPeerSatisfactionEdges,
  getPeerSatisfactionEdgesToSkip,
  pruneDanglingPeerSatisfactionEdges,
} from '@pnpm/lockfile.peer-edges'
import type { LockfileObject, PackageSnapshot, PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

const RESOLUTION = { integrity: 'sha512-AAAA' }

const OPTIONAL_PEERS_PKG: PackageSnapshot = {
  resolution: RESOLUTION,
  peerDependencies: { 'peer-a': '^1.0.0', 'peer-c': '^1.0.0' },
  peerDependenciesMeta: { 'peer-c': { optional: true } },
  dependencies: { 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
}

const PKG = 'abc@1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' as DepPath

function lockfile (importers: Record<string, Partial<ProjectSnapshot>>, packages: Record<string, PackageSnapshot>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: Object.fromEntries(Object.entries(importers).map(([id, importer]) => [id, { specifiers: {}, ...importer }])) as Record<ProjectId, ProjectSnapshot>,
    packages: packages as PackageSnapshots,
  }
}

function leaf (): PackageSnapshot {
  return { resolution: RESOLUTION }
}

describe('getPeerSatisfactionEdges', () => {
  test('classifies only an optional peer that every reaching importer lists', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
        devDependencies: { 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
      },
    }, {
      [PKG]: OPTIONAL_PEERS_PKG,
      'peer-a@1.0.0': leaf(),
      'peer-c@1.0.0': leaf(),
    }))
    expect(edges.get(PKG)).toStrictEqual(new Set(['peer-c']))
  })

  test('follows an edge whose target no importer lists', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
      },
    }, {
      [PKG]: OPTIONAL_PEERS_PKG,
      'peer-a@1.0.0': leaf(),
      'peer-c@1.0.0': leaf(),
    }))
    expect(edges.size).toBe(0)
  })

  test('follows an edge when an ancestor package provides the peer', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { parent: '1.0.0' },
        devDependencies: { 'peer-c': '1.0.0' },
      },
    }, {
      'parent@1.0.0': {
        resolution: RESOLUTION,
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)', 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
      },
      [PKG]: OPTIONAL_PEERS_PKG,
      'peer-a@1.0.0': leaf(),
      'peer-c@1.0.0': leaf(),
    }))
    // The importer lists peer-c too, but it does not provide it to abc: the
    // parent package does, and that edge is always followed.
    expect(edges.get(PKG)).toStrictEqual(new Set(['peer-c']))
    expect(edges.has('parent@1.0.0' as DepPath)).toBe(false)
  })

  test('follows an edge when another importer reaches the dependent without listing the peer', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      'project-1': {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
        devDependencies: { 'peer-c': '1.0.0' },
      },
      'project-2': {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
      },
    }, {
      [PKG]: OPTIONAL_PEERS_PKG,
      'peer-a@1.0.0': leaf(),
      'peer-c@1.0.0': leaf(),
    }))
    expect(edges.size).toBe(0)
  })

  test('counts the workspace root as listing the peer only when resolvePeersFromWorkspaceRoot is on', () => {
    const workspaceLockfile = lockfile({
      '.': {
        devDependencies: { 'peer-c': '1.0.0' },
      },
      'project-1': {
        dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
      },
    }, {
      [PKG]: OPTIONAL_PEERS_PKG,
      'peer-a@1.0.0': leaf(),
      'peer-c@1.0.0': leaf(),
    })
    expect(getPeerSatisfactionEdges(workspaceLockfile, { resolvePeersFromWorkspaceRoot: true }).get(PKG)).toStrictEqual(new Set(['peer-c']))
    expect(getPeerSatisfactionEdges(workspaceLockfile, { resolvePeersFromWorkspaceRoot: false }).size).toBe(0)
    expect(getPeerSatisfactionEdges(workspaceLockfile).size).toBe(0)
  })

  test('does not count a production dependency of the workspace root as listing the peer', () => {
    for (const field of ['dependencies', 'optionalDependencies'] as const) {
      const edges = getPeerSatisfactionEdges(lockfile({
        '.': {
          [field]: { 'peer-c': '1.0.0' },
        },
        'project-1': {
          dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
        },
      }, {
        [PKG]: OPTIONAL_PEERS_PKG,
        'peer-a@1.0.0': leaf(),
        'peer-c@1.0.0': leaf(),
      }), { resolvePeersFromWorkspaceRoot: true })
      expect(edges.size).toBe(0)
    }
  })

  test('does not treat an alias marked optional in peerDependenciesMeta but absent from peerDependencies as a peer', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { abc: '1.0.0' },
        devDependencies: { 'peer-c': '1.0.0' },
      },
    }, {
      'abc@1.0.0': {
        resolution: RESOLUTION,
        peerDependenciesMeta: { 'peer-c': { optional: true } },
        dependencies: { 'peer-c': '1.0.0' },
      },
      'peer-c@1.0.0': leaf(),
    }))
    expect(edges.size).toBe(0)
  })

  test('does not match Object.prototype property names as peers', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { abc: '1.0.0' },
        devDependencies: { constructor: '1.0.0', toString: '1.0.0' },
      },
    }, {
      'abc@1.0.0': {
        resolution: RESOLUTION,
        peerDependencies: { 'peer-c': '^1.0.0' },
        peerDependenciesMeta: { 'peer-c': { optional: true } },
        dependencies: { constructor: '1.0.0', toString: '1.0.0' },
      },
      'constructor@1.0.0': leaf(),
      'toString@1.0.0': leaf(),
    }))
    expect(edges.size).toBe(0)
  })

  test('classifies optional peers named after Object.prototype properties', () => {
    const edges = getPeerSatisfactionEdges(lockfile({
      '.': {
        dependencies: { abc: '1.0.0(constructor@1.0.0)(toString@1.0.0)' },
        devDependencies: { constructor: '1.0.0', toString: '1.0.0' },
      },
    }, {
      'abc@1.0.0(constructor@1.0.0)(toString@1.0.0)': {
        resolution: RESOLUTION,
        peerDependencies: { constructor: '^1.0.0', toString: '^1.0.0' },
        peerDependenciesMeta: { constructor: { optional: true as const }, toString: { optional: true as const } },
        dependencies: { constructor: '1.0.0', toString: '1.0.0' },
      },
      'constructor@1.0.0': leaf(),
      'toString@1.0.0': leaf(),
    }))
    expect(edges.get('abc@1.0.0(constructor@1.0.0)(toString@1.0.0)' as DepPath)).toStrictEqual(new Set(['constructor', 'toString']))
  })

  test('does not share a walk between targets whose non-listing importers differ', () => {
    // The importers that do not list x are 1, 2 and 3, and those that do not
    // list y are 1 and 23. Without a separator, both sets would join to "123".
    const importers: Record<string, Partial<ProjectSnapshot>> = {}
    for (let index = 0; index < 24; index++) {
      importers[`project-${String(index).padStart(2, '0')}`] = {
        devDependencies: {
          ...([1, 2, 3].includes(index) ? {} : { x: '1.0.0' }),
          ...([1, 23].includes(index) ? {} : { y: '1.0.0' }),
        },
      }
    }
    importers['project-02'].dependencies = { p: '1.0.0(x@1.0.0)' }
    importers['project-23'].dependencies = { q: '1.0.0(y@1.0.0)' }
    const edges = getPeerSatisfactionEdges(lockfile(importers, {
      'p@1.0.0(x@1.0.0)': {
        resolution: RESOLUTION,
        peerDependencies: { x: '^1.0.0' },
        peerDependenciesMeta: { x: { optional: true } },
        dependencies: { x: '1.0.0' },
      },
      'q@1.0.0(y@1.0.0)': {
        resolution: RESOLUTION,
        peerDependencies: { y: '^1.0.0' },
        peerDependenciesMeta: { y: { optional: true } },
        dependencies: { y: '1.0.0' },
      },
      'x@1.0.0': leaf(),
      'y@1.0.0': leaf(),
    }))
    // project-02 reaches p without listing x, and project-23 reaches q
    // without listing y, so both edges are followed.
    expect(edges.size).toBe(0)
  })
})

describe('getPeerSatisfactionEdgesToSkip', () => {
  const prodLockfile = lockfile({
    '.': {
      dependencies: { abc: '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)' },
      devDependencies: { 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
    },
  }, {
    [PKG]: OPTIONAL_PEERS_PKG,
    'peer-a@1.0.0': leaf(),
    'peer-c@1.0.0': leaf(),
  })

  test('skips nothing when every dependency group is included', () => {
    expect(getPeerSatisfactionEdgesToSkip(prodLockfile, {})).toBeUndefined()
    expect(getPeerSatisfactionEdgesToSkip(prodLockfile, {
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })).toBeUndefined()
  })

  test('skips the edges when a dependency group is excluded', () => {
    for (const excluded of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
      const include = { dependencies: true, devDependencies: true, optionalDependencies: true, [excluded]: false }
      expect(getPeerSatisfactionEdgesToSkip(prodLockfile, { include })?.get(PKG)).toStrictEqual(new Set(['peer-c']))
    }
  })
})

describe('pruneDanglingPeerSatisfactionEdges', () => {
  test('drops an edge whose target is not retained, without mutating the input', () => {
    const snapshot: PackageSnapshot = {
      resolution: RESOLUTION,
      peerDependencies: { 'peer-c': '^1.0.0' },
      peerDependenciesMeta: { 'peer-c': { optional: true } },
      optionalDependencies: { 'peer-c': '1.0.0' },
    }
    const packages = { ['abc@1.0.0(peer-c@1.0.0)' as DepPath]: snapshot }
    const edges = new Map([['abc@1.0.0(peer-c@1.0.0)' as DepPath, new Set(['peer-c'])]])
    const result = pruneDanglingPeerSatisfactionEdges(packages, edges)
    expect(result['abc@1.0.0(peer-c@1.0.0)' as DepPath].optionalDependencies).toBeUndefined()
    expect(snapshot.optionalDependencies).toStrictEqual({ 'peer-c': '1.0.0' })
    expect(packages['abc@1.0.0(peer-c@1.0.0)' as DepPath]).toBe(snapshot)
  })

  test('keeps an edge whose target is retained through another path', () => {
    const packages: PackageSnapshots = {
      ['abc@1.0.0(peer-c@1.0.0)' as DepPath]: {
        resolution: RESOLUTION,
        dependencies: { 'peer-c': '1.0.0' },
      },
      ['peer-c@1.0.0' as DepPath]: leaf(),
    }
    const edges = new Map([['abc@1.0.0(peer-c@1.0.0)' as DepPath, new Set(['peer-c'])]])
    expect(pruneDanglingPeerSatisfactionEdges(packages, edges)).toBe(packages)
  })
})
