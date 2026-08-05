import { expect, test } from '@jest/globals'

import { parseWantedDependencies } from '../src/parseWantedDependencies.js'

const defaults = {
  allowNew: true,
  defaultTag: 'latest',
  dev: false,
  devDependencies: {},
  optional: false,
  optionalDependencies: {},
}

test('a requested version that the kept range excludes is reported instead of applied', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@7.8.5'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    keepManifestSpecifiers: true,
  })

  expect(wantedDependencies).toStrictEqual([])
  expect(outsideKeptRange).toStrictEqual([{ alias: 'semver', requested: '7.8.5', kept: '^6.0.0' }])
})

test('a requested range that merely overlaps the kept range is reported instead of applied', () => {
  // Resolution would pick a version above `^6.0.0`, which the lockfile importer entry may not record.
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@>=6'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    keepManifestSpecifiers: true,
  })

  expect(wantedDependencies).toStrictEqual([])
  expect(outsideKeptRange).toStrictEqual([{ alias: 'semver', requested: '>=6', kept: '^6.0.0' }])
})

test('a requested version inside the kept range is applied', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@6.3.0'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    keepManifestSpecifiers: true,
  })

  expect(wantedDependencies).toHaveLength(1)
  expect(wantedDependencies[0].bareSpecifier).toBe('6.3.0')
  expect(outsideKeptRange).toStrictEqual([])
})

test('specifiers that are not semver ranges are left to resolution', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@beta', 'foo@1.0.0'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0', foo: 'workspace:*' },
    keepManifestSpecifiers: true,
  })

  expect(wantedDependencies.map(({ bareSpecifier }) => bareSpecifier)).toStrictEqual(['beta', '1.0.0'])
  expect(outsideKeptRange).toStrictEqual([])
})

test('a new dependency has no kept range to stay inside of', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@7.8.5'], {
    ...defaults,
    currentBareSpecifiers: {},
    keepManifestSpecifiers: true,
  })

  expect(wantedDependencies).toHaveLength(1)
  expect(outsideKeptRange).toStrictEqual([])
})

test('the kept range is not enforced when the manifest is rewritten', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@7.8.5'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
  })

  expect(wantedDependencies).toHaveLength(1)
  expect(wantedDependencies[0].bareSpecifier).toBe('7.8.5')
  expect(outsideKeptRange).toStrictEqual([])
})
