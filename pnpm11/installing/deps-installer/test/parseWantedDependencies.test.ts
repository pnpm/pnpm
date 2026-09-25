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
    readonlyManifest: true,
  })

  expect(wantedDependencies).toStrictEqual([])
  expect(outsideKeptRange).toStrictEqual([{ alias: 'semver', requested: '7.8.5', kept: '^6.0.0' }])
})

test('a requested range is superseded by the kept range', () => {
  // Range-against-range containment is not decided consistently across semver implementations,
  // so the specifier the importer entry will record is what resolution gets.
  const { wantedDependencies, outsideKeptRange, supersededByKeptRange } = parseWantedDependencies(['semver@>=6'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    readonlyManifest: true,
  })

  expect(wantedDependencies.map(({ bareSpecifier }) => bareSpecifier)).toStrictEqual(['^6.0.0'])
  expect(outsideKeptRange).toStrictEqual([])
  expect(supersededByKeptRange).toStrictEqual([{ alias: 'semver', requested: '>=6', kept: '^6.0.0' }])
})

test('a requested prerelease is judged by the range that admits it', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@2.0.0-beta.1'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^2.0.0-0' },
    readonlyManifest: true,
  })

  expect(wantedDependencies.map(({ bareSpecifier }) => bareSpecifier)).toStrictEqual(['2.0.0-beta.1'])
  expect(outsideKeptRange).toStrictEqual([])
})

test('a requested version inside the kept range is applied', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@6.3.0'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    readonlyManifest: true,
  })

  expect(wantedDependencies).toHaveLength(1)
  expect(wantedDependencies[0].bareSpecifier).toBe('6.3.0')
  expect(outsideKeptRange).toStrictEqual([])
})

test('a dist tag, and a kept specifier that is no semver range, are superseded too', () => {
  const { wantedDependencies, outsideKeptRange, supersededByKeptRange } = parseWantedDependencies(['semver@beta', 'foo@1.0.0'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0', foo: 'workspace:*' },
    readonlyManifest: true,
  })

  expect(wantedDependencies.map(({ bareSpecifier }) => bareSpecifier)).toStrictEqual(['^6.0.0', 'workspace:*'])
  expect(outsideKeptRange).toStrictEqual([])
  expect(supersededByKeptRange).toStrictEqual([
    { alias: 'semver', requested: 'beta', kept: '^6.0.0' },
    { alias: 'foo', requested: '1.0.0', kept: 'workspace:*' },
  ])
})

test('a selector without a version is honored as-is', () => {
  const { wantedDependencies, outsideKeptRange, supersededByKeptRange } = parseWantedDependencies(['semver'], {
    ...defaults,
    currentBareSpecifiers: { semver: '^6.0.0' },
    readonlyManifest: true,
  })

  expect(wantedDependencies.map(({ bareSpecifier }) => bareSpecifier)).toStrictEqual(['^6.0.0'])
  expect(outsideKeptRange).toStrictEqual([])
  expect(supersededByKeptRange).toStrictEqual([])
})

test('a new dependency has no kept range to stay inside of', () => {
  const { wantedDependencies, outsideKeptRange } = parseWantedDependencies(['semver@7.8.5'], {
    ...defaults,
    currentBareSpecifiers: {},
    readonlyManifest: true,
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

test('readonly aliases use their hook-provided specifiers', () => {
  const { wantedDependencies, outsideKeptRange, supersededByKeptRange } = parseWantedDependencies([
    'hook-owned@2.0.0',
    'declared@2.0.0',
  ], {
    ...defaults,
    currentBareSpecifiers: {
      'hook-owned': '^1.0.0',
      declared: '^1.0.0',
    },
    readonlySpecifiers: {
      'hook-owned': '^1.0.0',
    },
  })

  expect(wantedDependencies).toStrictEqual([
    {
      alias: 'hook-owned',
      bareSpecifier: '^1.0.0',
      dev: false,
      optional: false,
      prevSpecifier: '^1.0.0',
      saveCatalogName: undefined,
    },
    {
      alias: 'declared',
      bareSpecifier: '2.0.0',
      dev: false,
      optional: false,
      prevSpecifier: '^1.0.0',
      saveCatalogName: undefined,
    },
  ])
  expect(outsideKeptRange).toStrictEqual([])
  expect(supersededByKeptRange).toStrictEqual([{
    alias: 'hook-owned',
    requested: '2.0.0',
    kept: '^1.0.0',
  }])
})

test('readonly aliases preserve empty hook-provided specifiers', () => {
  const { wantedDependencies, supersededByKeptRange } = parseWantedDependencies(['hook-owned@2.0.0'], {
    ...defaults,
    currentBareSpecifiers: {},
    readonlySpecifiers: {
      'hook-owned': '',
    },
  })

  expect(wantedDependencies).toStrictEqual([{
    alias: 'hook-owned',
    bareSpecifier: '',
    dev: false,
    optional: false,
    prevSpecifier: '',
    saveCatalogName: undefined,
  }])
  expect(supersededByKeptRange).toStrictEqual([{
    alias: 'hook-owned',
    requested: '2.0.0',
    kept: '',
  }])
})

test('readonly specifiers are used when current specifiers are ignored', () => {
  const { wantedDependencies, supersededByKeptRange } = parseWantedDependencies(['hook-owned@1.0.1'], {
    ...defaults,
    currentBareSpecifiers: {},
    readonlySpecifiers: {
      'hook-owned': '^1.0.0',
    },
  })

  expect(wantedDependencies[0].bareSpecifier).toBe('^1.0.0')
  expect(supersededByKeptRange).toStrictEqual([{
    alias: 'hook-owned',
    requested: '1.0.1',
    kept: '^1.0.0',
  }])
})

test('readonly aliases are allowed when new dependencies are disabled', () => {
  const { wantedDependencies } = parseWantedDependencies(['hook-owned'], {
    ...defaults,
    allowNew: false,
    currentBareSpecifiers: {},
    readonlySpecifiers: {
      'hook-owned': '',
    },
  })

  expect(wantedDependencies).toStrictEqual([{
    alias: 'hook-owned',
    bareSpecifier: '',
    dev: false,
    optional: false,
    prevSpecifier: '',
    saveCatalogName: undefined,
  }])
})

test('an alias a hook removes is dropped and reported', () => {
  const { wantedDependencies, removedByHook } = parseWantedDependencies(['hook-removed@2.0.0', 'semver@7.8.5'], {
    ...defaults,
    currentBareSpecifiers: {},
    hookRemovedAliases: new Set(['hook-removed']),
  })

  expect(wantedDependencies.map(({ alias }) => alias)).toStrictEqual(['semver'])
  expect(removedByHook).toStrictEqual(['hook-removed'])
})
