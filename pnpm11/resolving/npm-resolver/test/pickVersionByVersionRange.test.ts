import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { EXISTING_VERSION_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'
import semver from 'semver'

import {
  pickLowestVersionByVersionRange,
  pickStableCachedRangeVersion,
  pickVersionByVersionRange,
} from '../src/pickPackageFromMeta.js'

// Deliberately mixes prereleases, build metadata, an unparseable entry, and a
// version only a loose parse accepts, because the pickers select over whatever
// keys a registry puts in `versions`.
const VERSIONS = [
  '0.0.1',
  '1.0.0-alpha.1',
  '1.0.0-beta.2',
  '1.0.0',
  '1.0.10',
  '1.2.0',
  '1.2.3+build.1',
  '1.10.0',
  '2.0.0-rc.1',
  '2.0.0',
  '3.0.0',
  'v4.0.0',
  'not-a-version',
]

const RANGES = [
  '*',
  '^1.0.0',
  '~1.2.0',
  '1.x',
  '>=1.0.0 <2.0.0',
  '1.0.0 || 3.0.0',
  '>=1.0.0-alpha.1 <2.0.0',
  '2.0.0-rc.1',
  '<=1.2',
  '4.0.0',
  '^9.0.0',
  'not-a-range',
]

// No `latest` tag, so the pickers can't take their dist-tag shortcut and every
// range goes through the version scan the semver calls used to do.
function metaWithoutLatest (versions: string[]): PackageMeta {
  const meta: PackageMeta = {
    name: 'pick-version',
    'dist-tags': {},
    versions: {},
  }
  for (const version of versions) {
    meta.versions[version] = {
      name: 'pick-version',
      version,
      dist: { tarball: `https://registry.npmjs.org/pick-version/-/pick-version-${version}.tgz`, shasum: '' },
    }
  }
  return meta
}

test.each(RANGES)('the highest satisfying version of %s matches semver.maxSatisfying', (versionRange) => {
  const meta = metaWithoutLatest(VERSIONS)
  expect(pickVersionByVersionRange({ meta, versionRange })).toBe(semver.maxSatisfying(VERSIONS, versionRange, true))
})

test.each(RANGES.filter((versionRange) => versionRange !== '*'))(
  'the lowest satisfying version of %s matches semver.minSatisfying',
  (versionRange) => {
    const meta = metaWithoutLatest(VERSIONS)
    expect(pickLowestVersionByVersionRange({ meta, versionRange })).toBe(semver.minSatisfying(VERSIONS, versionRange, true))
  }
)

test('a range satisfied by equal versions keeps the first of them, as semver does', () => {
  // `1.2.3+build.1` and `1.2.3+build.2` compare equal (build metadata is
  // ignored), so the winner is decided by which one is seen first.
  const versions = ['1.2.3+build.1', '1.2.3+build.2']
  const meta = metaWithoutLatest(versions)
  expect(pickVersionByVersionRange({ meta, versionRange: '1.2.3' }))
    .toBe(semver.maxSatisfying(versions, '1.2.3', true))
  expect(pickLowestVersionByVersionRange({ meta, versionRange: '1.2.3' }))
    .toBe(semver.minSatisfying(versions, '1.2.3', true))
})

test('a packument of only unparseable versions satisfies nothing', () => {
  const meta = metaWithoutLatest(['not-a-version', 'also-not-a-version'])
  expect(pickVersionByVersionRange({ meta, versionRange: '^1.0.0' })).toBeNull()
  expect(pickLowestVersionByVersionRange({ meta, versionRange: '^1.0.0' })).toBeNull()
})

test('a single dominant lockfile version makes a cached range stable', () => {
  const meta = metaWithoutLatest(['1.0.0', '1.1.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '^1.0.0': { selectorType: 'range', weight: 1000 },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBe('1.0.0')
})

test('a cache missing the lockfile version is not stable', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.1.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('multiple satisfying lockfile versions are not treated as stable', () => {
  const meta = metaWithoutLatest(['1.0.0', '1.1.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '1.1.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('a competing selector that can tie the lockfile pin is not stable', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '>=1.1.0': { selectorType: 'range', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('a negative preferred selector disables cached range reuse', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '>=1.1.0': { selectorType: 'range', weight: -1 },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('a movable dist-tag cannot outweigh a dominant lockfile pin', () => {
  const meta = metaWithoutLatest(['1.0.0', '1.1.0'])
  meta['dist-tags'].next = '1.1.0'
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      next: { selectorType: 'tag', weight: EXISTING_VERSION_SELECTOR_WEIGHT - 1 },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBe('1.0.0')
})

test('a fractional selector weight disables cached range reuse', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '>=1.1.0': { selectorType: 'range', weight: 0.5 },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('a non-finite selector weight disables cached range reuse', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
      '>=1.1.0': { selectorType: 'range', weight: Number.POSITIVE_INFINITY },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})

test('selector weights whose sum is unsafe disable cached range reuse', () => {
  const meta = metaWithoutLatest(['1.0.0'])
  const result = pickStableCachedRangeVersion({
    meta,
    preferredVersionSelectors: {
      '1.0.0': { selectorType: 'version', weight: Number.MAX_SAFE_INTEGER },
      '>=1.0.0': { selectorType: 'range', weight: 1 },
    },
    versionRange: '^1.0.0',
  })

  expect(result).toBeNull()
})
