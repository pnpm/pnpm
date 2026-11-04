import { expect, test } from '@jest/globals'
import {
  findMovedRegistryKey,
  findOutdatedRegistryKey,
  keyPrefixForAlias,
  type LockfileObject,
  registryOfDepPath,
  rekeyDepPath,
} from '@pnpm/lockfile.utils'
import type { RegistryContext } from '@pnpm/types'

const WORK_REGISTRY = 'https://npm.example.com/'
const INTEGRITY = 'sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=='

/** The `@work` scope routes to the registry the declared prefixes name. */
function context (registriesByPrefix: Record<string, string>): RegistryContext {
  return {
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@work': WORK_REGISTRY,
    },
    registriesByPrefix,
  }
}

/** `@work/foo` is keyed plainly, `bar` by the `work` prefix. */
function lockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      ['@work/foo@1.0.0' as never]: { resolution: { integrity: INTEGRITY } },
      ['bar@work:1.0.0' as never]: { resolution: { integrity: INTEGRITY } },
    },
  } as LockfileObject
}

test('registryOfDepPath() reads the registry a key names', () => {
  const registries = context({ work: WORK_REGISTRY })
  expect(registryOfDepPath(registries, 'bar@work:1.0.0')).toBe(WORK_REGISTRY)
  expect(registryOfDepPath(registries, '@work/foo@1.0.0')).toBe(WORK_REGISTRY)
  expect(registryOfDepPath(registries, 'bar@1.0.0')).toBe('https://registry.npmjs.org/')
  expect(registryOfDepPath(registries, 'bar@npmjs:1.0.0')).toBe('https://registry.npmjs.org/')
  expect(registryOfDepPath(registries, 'bar@unknown:1.0.0')).toBeUndefined()
})

test('keyPrefixForAlias() names the prefix the keys of the alias\'s registry carry', () => {
  const registries = context({ zeta: WORK_REGISTRY, work: 'https://NPM.example.com' })
  expect(keyPrefixForAlias(registries, 'zeta', 'bar')).toBe('work')
  expect(keyPrefixForAlias(registries, 'work', 'bar')).toBe('work')
  // A built-in alias nothing declares keeps its own name off the default registry.
  expect(keyPrefixForAlias(registries, 'gh', 'bar')).toBe('gh')
  expect(keyPrefixForAlias(registries, 'unknown', 'bar')).toBeUndefined()
})

// https://github.com/pnpm/pnpm/issues/17575
test('keyPrefixForAlias() keeps a plain key for a package of the default registry', () => {
  expect(keyPrefixForAlias(context({}), 'npmjs', 'bar')).toBeUndefined()
  expect(keyPrefixForAlias(context({ npmjs: 'https://registry.npmjs.org/' }), 'npmjs', 'bar')).toBeUndefined()
  // A plain range of `@work/foo` routes elsewhere, so the plain key would name
  // the wrong registry.
  expect(keyPrefixForAlias(context({}), 'npmjs', '@work/foo')).toBe('npmjs')
})

test('rekeyDepPath() qualifies a plain key of a registry that declares a prefix', () => {
  const registries = context({ work: WORK_REGISTRY })
  expect(rekeyDepPath(registries, '@work/foo@1.0.0')).toBe('@work/foo@work:1.0.0')
  expect(rekeyDepPath(registries, '@work/foo@1.0.0(peer@1.0.0)')).toBe('@work/foo@work:1.0.0(peer@1.0.0)')
  expect(rekeyDepPath(registries, 'bar@work:1.0.0')).toBeUndefined()
  expect(rekeyDepPath(registries, 'bar@1.0.0')).toBeUndefined()
  expect(rekeyDepPath(registries, '@work/foo@file:../foo')).toBeUndefined()
})

test('rekeyDepPath() moves a key to the smallest prefix of its registry', () => {
  const registries = context({ zeta: WORK_REGISTRY, alpha: WORK_REGISTRY })
  expect(rekeyDepPath(registries, 'bar@zeta:1.0.0')).toBe('bar@alpha:1.0.0')
  expect(rekeyDepPath(registries, 'bar@alpha:1.0.0')).toBeUndefined()
})

test('rekeyDepPath() makes a qualified key of a default-registry package plain', () => {
  expect(rekeyDepPath(context({}), 'bar@npmjs:1.0.0(peer@1.0.0)')).toBe('bar@1.0.0(peer@1.0.0)')
  expect(rekeyDepPath(context({ work: 'https://registry.npmjs.org/' }), 'bar@work:1.0.0')).toBe('bar@1.0.0')
  expect(rekeyDepPath(context({}), '@work/foo@npmjs:1.0.0')).toBeUndefined()
})

test('rekeyDepPath() leaves the built-in aliases off the default registry and undeclared prefixes alone', () => {
  const registries = context({})
  expect(rekeyDepPath({ ...registries, registriesByScope: { default: 'https://registry.npmjs.org/' } }, 'bar@1.0.0')).toBeUndefined()
  expect(rekeyDepPath(registries, 'bar@gh:1.0.0')).toBeUndefined()
  expect(rekeyDepPath(registries, 'bar@unknown:1.0.0')).toBeUndefined()
})

test('findOutdatedRegistryKey() finds nothing when no prefix is declared', () => {
  expect(findOutdatedRegistryKey(lockfile(), context({}))).toBeUndefined()
})

test('findOutdatedRegistryKey() reports a qualified key of a default-registry package without a declared prefix', () => {
  const packages = { ...lockfile().packages, ['baz@npmjs:1.0.0' as never]: { resolution: { integrity: INTEGRITY } } }
  expect(findOutdatedRegistryKey({ ...lockfile(), packages }, context({}))).toStrictEqual({
    key: 'baz@npmjs:1.0.0',
    current: 'baz@1.0.0',
  })
})

test('findOutdatedRegistryKey() reports a plain key of a registry that declares a prefix', () => {
  expect(findOutdatedRegistryKey(lockfile(), context({ work: WORK_REGISTRY }))).toStrictEqual({
    key: '@work/foo@1.0.0',
    current: '@work/foo@work:1.0.0',
  })
})

test('findOutdatedRegistryKey() reports the smallest outdated key', () => {
  // `alpha` also names the registry, so `bar@work:1.0.0` is outdated too;
  // `@work/foo@1.0.0` sorts first.
  expect(findOutdatedRegistryKey(lockfile(), context({ work: WORK_REGISTRY, alpha: WORK_REGISTRY }))).toStrictEqual({
    key: '@work/foo@1.0.0',
    current: '@work/foo@alpha:1.0.0',
  })
})

/** `context()` after the `@work` scope moved from `OLD_REGISTRY`. */
const OLD_REGISTRY = 'https://old.example.com/'

function movedContext (): RegistryContext {
  return {
    ...context({}),
    lockedRegistriesByScope: { '@work': OLD_REGISTRY },
  }
}

test('registryOfDepPath() reads a plain key as resolved from the registry its scope was routed to', () => {
  const registries = movedContext()
  expect(registryOfDepPath(registries, '@work/foo@1.0.0')).toBe(OLD_REGISTRY)
  // An unscoped package, and a scope the record has no route for, resolved
  // from the default registry.
  expect(registryOfDepPath(registries, 'bar@1.0.0')).toBe('https://registry.npmjs.org/')
  expect(registryOfDepPath(registries, '@other/foo@1.0.0')).toBe('https://registry.npmjs.org/')
  // A qualified key names its registry.
  expect(registryOfDepPath({ ...registries, registriesByPrefix: { work: WORK_REGISTRY } }, '@work/foo@work:1.0.0')).toBe(WORK_REGISTRY)
})

test('findMovedRegistryKey() finds nothing without a record of the locked routes', () => {
  expect(findMovedRegistryKey(lockfile(), context({}))).toBeUndefined()
  expect(findMovedRegistryKey(lockfile(), { ...context({}), lockedRegistriesByScope: context({}).registriesByScope })).toBeUndefined()
})

test('findMovedRegistryKey() finds the smallest plain key of a scope routed elsewhere', () => {
  expect(findMovedRegistryKey(lockfile(), movedContext())).toStrictEqual({
    key: '@work/foo@1.0.0',
    from: OLD_REGISTRY,
    to: WORK_REGISTRY,
  })
  // The `@work` scope had no route of its own: its packages resolved from the
  // default registry, which it no longer routes to.
  expect(findMovedRegistryKey(lockfile(), { ...context({}), lockedRegistriesByScope: {} })).toStrictEqual({
    key: '@work/foo@1.0.0',
    from: 'https://registry.npmjs.org/',
    to: WORK_REGISTRY,
  })
})
