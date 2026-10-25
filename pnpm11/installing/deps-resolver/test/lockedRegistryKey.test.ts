import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { RegistryContext } from '@pnpm/types'

import { lockedVersionSatisfies } from '../lib/dependencyManifest.js'
import { getDepsToResolve } from '../lib/getDepsToResolve.js'
import { lockedKeyIsCurrent } from '../lib/lockedRegistryKey.js'

const WORK_REGISTRY = 'https://npm.example.test/'
const INTEGRITY = 'sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=='

/**
 * The `work` and `zeta` prefixes both name one registry, which the `@work`
 * scope also routes to. `gh` is the built-in alias, which nothing declares.
 */
const prefixedRegistries: RegistryContext = {
  registriesByScope: {
    default: 'https://registry.example.test/',
    '@work': WORK_REGISTRY,
  },
  registriesByPrefix: {
    work: WORK_REGISTRY,
    zeta: 'https://NPM.example.test',
  },
}

function satisfies (alias: string, bareSpecifier: string, depPath: string, registries: RegistryContext = prefixedRegistries): boolean | undefined {
  const [name, version] = splitDepPath(depPath)
  const registryName = version.includes(':') ? version.slice(0, version.indexOf(':')) : undefined
  return lockedVersionSatisfies({ alias, bareSpecifier }, {
    name,
    version: registryName == null ? version : version.slice(registryName.length + 1),
    registryName,
  }, registries)
}

function splitDepPath (depPath: string): [string, string] {
  const index = depPath.indexOf('@', 1)
  return [depPath.slice(0, index), depPath.slice(index + 1)]
}

test('lockedVersionSatisfies() admits a named-registry spec for its registry and package', () => {
  const key = '@scope/old@work:1.2.0'
  // The alias carries the package name of a bare range.
  expect(satisfies('@scope/old', 'work:^1.0.0', key)).toBe(true)
  expect(satisfies('@scope/old', 'work:@scope/old@^1.0.0', key)).toBe(true)
  // Reusing the entry would install `@scope/old` for a dependency that asked for `@scope/new`.
  expect(satisfies('@scope/old', 'work:@scope/new@^1.0.0', key)).toBe(false)
  // A spec aimed at another registry never satisfies the key. Another prefix
  // of the same registry does.
  expect(satisfies('@scope/old', 'gh:^1.0.0', key)).toBe(false)
  expect(satisfies('@scope/old', 'zeta:^1.0.0', key)).toBe(true)
  expect(satisfies('@scope/old', 'work:^2.0.0', key)).toBe(false)
})

test('lockedVersionSatisfies() admits a bare named-registry spec only for the alias package', () => {
  expect(satisfies('foo', 'work:^1.0.0', 'bar@work:1.2.0')).toBe(false)
  expect(satisfies('foo', 'work:bar@^1.0.0', 'bar@work:1.2.0')).toBe(true)
  expect(satisfies('foo', 'work:^1.0.0', 'foo@work:1.2.0')).toBe(true)
})

test('lockedVersionSatisfies() admits a plain range through the registry its scope routes to', () => {
  // `@work` routes to the registry `work:` names, so either key satisfies a
  // range, and a plain key satisfies the prefixed spec.
  expect(satisfies('@work/foo', '^1.0.0', '@work/foo@work:1.2.0')).toBe(true)
  expect(satisfies('@work/foo', '^1.0.0', '@work/foo@1.2.0')).toBe(true)
  expect(satisfies('@work/foo', 'work:^1.0.0', '@work/foo@1.2.0')).toBe(true)
  // A key of another registry never satisfies the range.
  expect(satisfies('@work/foo', '^1.0.0', '@work/foo@gh:1.2.0')).toBe(false)
  expect(satisfies('foo', '^1.0.0', 'foo@work:1.2.0')).toBe(false)
  expect(satisfies('foo', 'work:^1.0.0', 'foo@1.2.0')).toBe(false)
})

test('lockedVersionSatisfies() reads the registry of an npm alias target', () => {
  expect(satisfies('foo', 'npm:@work/foo@^1.0.0', '@work/foo@work:1.2.0')).toBe(true)
  expect(satisfies('foo', 'npm:@work/foo@^1.0.0', '@work/foo@1.2.0')).toBe(true)
  expect(satisfies('foo', 'npm:@work/bar@^1.0.0', '@work/foo@work:1.2.0')).toBe(false)
})

test('lockedVersionSatisfies() cannot judge a spec that is not a range', () => {
  expect(satisfies('foo', 'latest', 'foo@1.0.0')).toBeUndefined()
})

test('lockedKeyIsCurrent() holds when the key is the one the specifier mints', () => {
  const current = (depPath: string, bareSpecifier: string): boolean =>
    lockedKeyIsCurrent({ alias: '@work/foo', bareSpecifier }, depPath, prefixedRegistries)
  // The `@work` scope's registry declares `work`, so a plain range mints a `work:` key.
  expect(current('@work/foo@work:1.2.0', '^1.0.0')).toBe(true)
  expect(current('@work/foo@1.2.0', '^1.0.0')).toBe(false)
  // Another declared alias of the registry mints the same key.
  expect(current('@work/foo@work:1.2.0', 'zeta:^1.0.0')).toBe(true)
  expect(current('@work/foo@zeta:1.2.0', 'zeta:^1.0.0')).toBe(false)
  expect(current('@work/foo@zeta:1.2.0', 'work:^1.0.0')).toBe(false)
  // Keys that are not registry versions are left alone.
  expect(current('@work/foo@file:packages/foo', 'file:packages/foo')).toBe(true)
})

test('lockedKeyIsCurrent() follows the spelling of a built-in alias', () => {
  const registries: RegistryContext = {
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  }
  const current = (depPath: string, bareSpecifier: string): boolean =>
    lockedKeyIsCurrent({ alias: 'foo', bareSpecifier }, depPath, registries)
  expect(current('foo@1.0.0', '^1.0.0')).toBe(true)
  expect(current('foo@npmjs:1.0.0', 'npmjs:^1.0.0')).toBe(true)
  // Both entries come from registry.npmjs.org, so the version pin holds, but
  // neither key is the one the specifier mints.
  expect(satisfies('foo', 'npmjs:^1.0.0', 'foo@1.0.0', registries)).toBe(true)
  expect(current('foo@1.0.0', 'npmjs:^1.0.0')).toBe(false)
  expect(satisfies('foo', '^1.0.0', 'foo@npmjs:1.0.0', registries)).toBe(true)
  expect(current('foo@npmjs:1.0.0', '^1.0.0')).toBe(false)
  expect(current('foo@gh:1.0.0', 'gh:^1.0.0')).toBe(true)
})

/** A parent locked with its child under `childRef`. */
function lockfileWithChild (childRef: string, childDepPath: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      ['@work/parent@work:1.0.0' as never]: {
        resolution: { integrity: INTEGRITY },
        dependencies: { '@work/foo': childRef },
      },
      [childDepPath as never]: { resolution: { integrity: INTEGRITY } },
    },
  } as LockfileObject
}

test('a locked key the registries setting re-keys keeps its pin and is resolved again', () => {
  for (const [childRef, childDepPath] of [
    ['1.2.0', '@work/foo@1.2.0'],
    ['zeta:1.2.0', '@work/foo@zeta:1.2.0'],
  ]) {
    const [resolved] = getDepsToResolve([{ alias: '@work/foo', bareSpecifier: '^1.0.0', dev: false, optional: false }], lockfileWithChild(childRef, childDepPath), {
      ...prefixedRegistries,
      currentDepth: 1,
      prefix: '/project',
      proceed: false,
      resolvedDependencies: { '@work/foo': childRef },
    })
    expect(resolved.infoFromLockfile).toMatchObject({ depPath: childDepPath, version: '1.2.0' })
    expect(resolved.proceed).toBe(true)
  }
})

test('a current locked key keeps its pin without forcing the edge to be resolved again', () => {
  const [resolved] = getDepsToResolve([{ alias: '@work/foo', bareSpecifier: '^1.0.0', dev: false, optional: false }], lockfileWithChild('work:1.2.0', '@work/foo@work:1.2.0'), {
    ...prefixedRegistries,
    currentDepth: 1,
    prefix: '/project',
    proceed: false,
    resolvedDependencies: { '@work/foo': 'work:1.2.0' },
  })
  expect(resolved.infoFromLockfile).toMatchObject({ depPath: '@work/foo@work:1.2.0', version: '1.2.0' })
  expect(resolved.proceed).toBe(false)
})

test('a locked key from another registry does not pin the edge', () => {
  const [resolved] = getDepsToResolve([{ alias: '@work/foo', bareSpecifier: '^1.0.0', dev: false, optional: false }], lockfileWithChild('gh:1.2.0', '@work/foo@gh:1.2.0'), {
    ...prefixedRegistries,
    currentDepth: 1,
    prefix: '/project',
    proceed: false,
    resolvedDependencies: { '@work/foo': 'gh:1.2.0' },
  })
  expect(resolved.infoFromLockfile).toBeUndefined()
})
