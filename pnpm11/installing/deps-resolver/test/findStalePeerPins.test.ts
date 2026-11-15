import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { EXISTING_VERSION_SELECTOR_WEIGHT, type PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { DepPath, ProjectId } from '@pnpm/types'

import { collectDirectDependencySpecs, findStalePeerPins, releaseStalePeerPins } from '../lib/findStalePeerPins.js'

test.each(['1.0.0', 'v1.0.0', '=1.0.0', ' =v1.0.0 '])('releasing a pin removes the lockfile weight from the version that %p names', (spec) => {
  const lockfileWeighted = { selectorType: 'version' as const, weight: EXISTING_VERSION_SELECTOR_WEIGHT }
  const preferredVersions: PreferredVersions = {
    foo: { '1.0.0': lockfileWeighted, '2.0.0': lockfileWeighted, '3.0.0': lockfileWeighted },
  }
  const released = releaseStalePeerPins(new Map([['foo', '2.0.0']]), {
    directSpecsByName: new Map([['foo', new Set([spec])]]),
    preferredVersions,
    resolvedDependencies: { foo: '2.0.0' },
  })
  expect({ ...released.preferredVersions.foo }).toStrictEqual({ '3.0.0': lockfileWeighted })
  expect(released.resolvedDependencies).toStrictEqual({})
})

test.each([
  ['^2.0.0', ['2.0.0']],
  ['^1.0.0', []],
  ['2.0.0', []],
  ['catalog:', ['2.0.0']],
] as Array<[string, string[]]>)('collects only current range pins for %s', (specifier, expected) => {
  const id = '.' as ProjectId
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    packages: { ['provider@2.0.0' as DepPath]: { resolution: { tarball: 'https://registry.example.test/provider-2.tgz' } } },
    importers: { [id]: { specifiers: { provider: '^2.0.0' }, dependencies: { provider: '2.0.0' } } },
  }
  const specs = collectDirectDependencySpecs([
    { id, manifest: { dependencies: { provider: specifier } } },
    { id: 'shared' as ProjectId, manifest: { peerDependencies: { provider: '*' } } },
  ], { default: { provider: '^2.0.0' } }, lockfile)
  expect([...specs.rangePins.get('provider') ?? []]).toStrictEqual(expected)
})

test.each([
  [['^1.0.0', '^2.0.0'], false],
  [['^1.0.0', '2.0.0'], true],
] as Array<[string[], boolean]>)('a ranged provider keeps its peer pin unless an exact provider changes: %j', (ranges, stale) => {
  const pins = findStalePeerPins({ provider: '1.0.0' }, {
    directSpecsByName: new Map([['provider', new Set(ranges)]]),
    rangePins: new Map([['provider', new Set(['1.0.0'])]]),
    manifest: { peerDependencies: { provider: '*' } },
    lockfile: { lockfileVersion: '9.0', importers: {}, packages: { ['provider@1.0.0' as DepPath]: { resolution: { integrity: '' } } } },
  })
  expect(pins.has('provider')).toBe(stale)
})

test.each([{}, { provider: '*' }] as Array<Record<string, string>>)('does not read range pins for dependencies without a peer-only consumer: %j', (peerDependencies) => {
  const id = '.' as ProjectId
  const specs = collectDirectDependencySpecs([
    { id, manifest: { dependencies: { provider: '^1.0.0' }, peerDependencies } },
  ], {}, {
    lockfileVersion: '9.0',
    importers: {
      [id]: {
        specifiers: { provider: '^1.0.0' },
        get dependencies (): never {
          throw new Error('Unrelated pins should not be read')
        },
      },
    },
  })
  expect(specs.ranges.get('provider')).toStrictEqual(new Set(['^1.0.0']))
  expect(specs.rangePins.size).toBe(0)
})
