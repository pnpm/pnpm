import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import {
  createNpmResolver,
} from '@pnpm/resolving.npm-resolver'
import { EXISTING_VERSION_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'
import { fixtures } from '@pnpm/test-fixtures'
import type { RegistriesByScope } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

import { getPkgMirrorPath, prepareJsonForDisk, saveMeta } from '../src/pickPackage.js'
import { getMockAgent, retryLoadJsonFile, setupMockAgent, teardownMockAgent } from './utils/index.js'

const f = fixtures(import.meta.dirname)
/* eslint-disable @typescript-eslint/no-explicit-any */
const ghAcmePrivateMeta = loadJsonFileSync<any>(f.find('gh-acme-private.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const GH_REGISTRY = 'https://npm.pkg.github.com/'
const ENTERPRISE_REGISTRY = 'https://npm.enterprise.example.com/'

// The `@github` scope is no longer defaulted to GitHub Packages — so public
// `@github/*` npm installs are not hijacked. The `gh:` prefix resolves via
// the built-in `gh` named-registry alias instead.
const registriesByScope = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
} satisfies RegistriesByScope

const fetch = createFetchFromRegistry({})

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

function interceptGhAcmePrivate (registry: string = GH_REGISTRY): void {
  const slash = '%2F'
  const pool = getMockAgent().get(registry.replace(/\/$/, ''))
  pool.intercept({ path: `/@acme${slash}private`, method: 'GET' }).reply(200, ghAcmePrivateMeta)
}

test('resolveFromNamedRegistry() resolves a scoped package published to GitHub Packages via the built-in gh: alias', async () => {
  interceptGhAcmePrivate()

  const cacheDir = temporaryDirectory()
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    { calcSpecifier: true }
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'gh',
    id: '@acme/private@gh:2.1.0',
    latest: '2.1.0',
    manifest: {
      name: '@acme/private',
      version: '2.1.0',
    },
    resolution: {
      integrity: expect.any(String),
      tarball: 'https://npm.pkg.github.com/download/@acme/private/2.1.0/acme-private-2.1.0.tgz',
    },
    // When the alias matches the package name, the normalized specifier keeps the `gh:<range>` shape.
    normalizedBareSpecifier: 'gh:^2.1.0',
    alias: '@acme/private',
  })

  // The resolve function writes the cache asynchronously — wait briefly before reading.
  const meta = await retryLoadJsonFile<any>(path.join(cacheDir, ABBREVIATED_META_DIR, 'npm.pkg.github.com/@acme/private.jsonl')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(meta).toMatchObject({
    name: '@acme/private',
    versions: expect.any(Object),
    'dist-tags': expect.any(Object),
  })
})

test('resolveFromNamedRegistry() reaches the public registry through the built-in npmjs: alias when the default registry is elsewhere', async () => {
  // The point of the alias: `registry` is an internal proxy here, so nothing
  // else in the project would reach npmjs. `npm:` cannot do this — it is the
  // alias protocol and resolves through whatever `registry` points at.
  const slash = '%2F'
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: `/@acme${slash}private`, method: 'GET' }).reply(200, ghAcmePrivateMeta)

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope: {
      default: ENTERPRISE_REGISTRY,
      '@jsr': 'https://npm.jsr.io/',
    } satisfies RegistriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'npmjs:^2.0.0' },
    {}
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'npmjs',
    id: '@acme/private@npmjs:2.1.0',
  })
})

test('resolveFromNamedRegistry() revalidates cached ranges under trust downgrade protection', async () => {
  interceptGhAcmePrivate()
  const cacheDir = temporaryDirectory()
  await saveMeta(
    getPkgMirrorPath(cacheDir, ABBREVIATED_META_DIR, GH_REGISTRY, '@acme/private'),
    prepareJsonForDisk(ghAcmePrivateMeta, undefined)
  )
  const fetchedUrls: string[] = []
  const countingFetch: typeof fetch = async (url, opts) => {
    fetchedUrls.push(url.toString())
    return fetch(url, opts)
  }
  const { resolveFromNamedRegistry } = createNpmResolver(countingFetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir,
    registriesByScope,
  })

  await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    {
      preferredVersions: {
        '@acme/private': {
          '2.1.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
        },
      },
      trustPolicy: 'no-downgrade',
    }
  )

  expect(fetchedUrls).toEqual(['https://npm.pkg.github.com/@acme%2Fprivate'])
})

test('resolveFromNamedRegistry() lets a proxying org override the built-in npmjs alias', async () => {
  interceptGhAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    // Same escape hatch GHES users have for `gh`: an org that mirrors npmjs
    // points `npmjs` at the mirror so nothing reaches the public host.
    registriesByPrefix: normalizeRegistriesByPrefix({
      npmjs: ENTERPRISE_REGISTRY,
    }),
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'npmjs:^2.0.0' },
    {}
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'npmjs',
    id: '@acme/private@npmjs:2.1.0',
  })
})

test('resolveFromNamedRegistry() preserves the scoped package name when the alias is a different name', async () => {
  interceptGhAcmePrivate()

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: 'my-private', bareSpecifier: 'gh:@acme/private@^1.0.0' },
    { calcSpecifier: true }
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'gh',
    id: '@acme/private@gh:1.0.0',
    manifest: {
      name: '@acme/private',
      version: '1.0.0',
    },
    // A custom alias forces the `gh:<pkgName>@<range>` form so the install
    // record in package.json unambiguously pins the original GitHub Packages name.
    normalizedBareSpecifier: 'gh:@acme/private@^1.0.0',
    alias: '@acme/private',
  })
})

test('resolveFromNamedRegistry() looks up the auth header by the named registry URL', async () => {
  interceptGhAcmePrivate()

  const calls: string[] = []
  const { resolveFromNamedRegistry } = createNpmResolver(
    fetch,
    (registry) => {
      calls.push(registry)
      return 'Bearer secret-github-token'
    },
    {
      storeDir: temporaryDirectory(),
      cacheDir: temporaryDirectory(),
      registriesByScope,
    }
  )

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:2.0.0' },
    {}
  )

  // The resolver must ask for credentials for the configured GitHub Packages URL
  // (not the default npm registry) — this is what makes `//npm.pkg.github.com/:_authToken=...`
  // entries in a `.npmrc` take effect for `gh:` specifiers.
  expect(calls).toContain(GH_REGISTRY)
  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'gh',
    id: '@acme/private@gh:2.0.0',
  })
})

test('resolveFromNamedRegistry() honours a user-defined named registry from config', async () => {
  interceptGhAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      work: ENTERPRISE_REGISTRY,
    }),
  })

  // `work:` is a user-defined alias — parsing and the URL lookup come from
  // the resolver's merged named-registriesByScope map, not the scope registriesByScope.
  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'work:^2.0.0' },
    { calcSpecifier: true }
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'work',
    id: '@acme/private@work:2.1.0',
    normalizedBareSpecifier: 'work:^2.1.0',
  })
})

test('resolveFromNamedRegistry() allows user config to override the built-in gh alias (GHES)', async () => {
  interceptGhAcmePrivate(ENTERPRISE_REGISTRY)

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    // A GHES user points `gh` at their enterprise host; the built-in default is shadowed.
    registriesByPrefix: normalizeRegistriesByPrefix({
      gh: ENTERPRISE_REGISTRY,
    }),
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    {}
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'gh',
    id: '@acme/private@gh:2.1.0',
  })
})

test('creating the resolver throws when a user-defined registry URL is malformed', () => {
  // Catch typos at startup rather than as a confusing 404 during resolution.
  expect(() => createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({ work: 'npm.work.example.com' }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_URL' }))

  expect(() => createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({ work: 'ftp://npm.work.example.com/' }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_URL' }))
})

test('resolveFromNamedRegistry() returns null for specifiers whose prefix is not a configured alias', async () => {
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  // No fetch mock is registered — the test would fail if the resolver tried to hit the network.
  await expect(resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: '^1.0.0' }, {})).resolves.toBeNull()
  await expect(resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'npm:@acme/private@1.0.0' }, {})).resolves.toBeNull()
  await expect(resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'jsr:@acme/private' }, {})).resolves.toBeNull()
  // `work:` isn't configured here.
  await expect(resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'work:^1.0.0' }, {})).resolves.toBeNull()
})

test('resolveFromNamedRegistry() does not claim the github: git shortcut scheme', async () => {
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  // `github:` belongs to the git resolver (npm-package-arg spec); GitHub Packages uses the `gh:` alias.
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'github:owner/repo' }, {})).resolves.toBeNull()
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'github:owner/repo#main' }, {})).resolves.toBeNull()
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'github:@acme/foo' }, {})).resolves.toBeNull()
})

test('resolveFromNamedRegistry() returns null when no alias is provided for a bare version selector', async () => {
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  // Without any package alias, `gh:<version>` cannot map to a package name.
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'gh:2.0.0' }, {})).resolves.toBeNull()
})

test('resolveFromNamedRegistry() throws when the specifier names an invalid scoped package', async () => {
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  // Scope without a package name is always a bug — refuse with a specific error code.
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'gh:@acme' }, {})).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
  })
  await expect(resolveFromNamedRegistry({ bareSpecifier: 'gh:@acme@2.0.0' }, {})).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
  })
})

test('the same package name served by two registriesByScope does not collide in the in-memory metadata cache', async () => {
  // Both registriesByScope serve `@acme/private`, but point at different tarballs.
  interceptGhAcmePrivate(GH_REGISTRY)
  /* eslint-disable @typescript-eslint/no-explicit-any */
  const enterpriseMeta = JSON.parse(JSON.stringify(ghAcmePrivateMeta))
  for (const version of Object.values<any>(enterpriseMeta.versions)) {
    version.dist.tarball = version.dist.tarball.replace('https://npm.pkg.github.com', 'https://npm.enterprise.example.com')
  }
  /* eslint-enable @typescript-eslint/no-explicit-any */
  const slash = '%2F'
  getMockAgent().get(ENTERPRISE_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: `/@acme${slash}private`, method: 'GET' })
    .reply(200, enterpriseMeta)

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({ work: ENTERPRISE_REGISTRY }),
  })

  // Resolving from the gh registry first populates the shared in-memory cache.
  const ghResult = await resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'gh:2.0.0' }, {})
  expect(ghResult).toMatchObject({
    id: '@acme/private@gh:2.0.0',
    resolution: {
      tarball: 'https://npm.pkg.github.com/download/@acme/private/2.0.0/acme-private-2.0.0.tgz',
    },
  })

  // Resolving the same name from the enterprise registry must use that
  // registry's own metadata — not the cached gh packument, whose tarball would
  // point at the wrong host (a cross-registry confusion bug if the in-memory
  // cache key omitted the registry).
  const workResult = await resolveFromNamedRegistry({ alias: '@acme/private', bareSpecifier: 'work:2.0.0' }, {})
  expect(workResult).toMatchObject({
    id: '@acme/private@work:2.0.0',
    resolution: {
      tarball: 'https://npm.enterprise.example.com/download/@acme/private/2.0.0/acme-private-2.0.0.tgz',
    },
  })
})

test('resolveFromNamedRegistry() preserves vulnerability-avoidance range selectors even when updateRequested is true', async () => {
  // Security regression: the simple-registry picker (jsr + named registriesByScope)
  // must use the same `stripLockfileVersionPins` helper as the npm picker,
  // so a targeted update drops only the target's lockfile pins and keeps
  // range penalties (e.g. `pnpm audit --fix` vulnerability avoidance).
  // Without the helper, dropping all selectors lets the "fix" re-pick the
  // vulnerable highest-in-range version.
  interceptGhAcmePrivate()

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    {
      preferredVersions: {
        '@acme/private': {
          // The target's own lockfile pin — dropped so it can't hold the
          // target at its old version.
          '2.0.0': { selectorType: 'version', weight: 1_000_000 },
          // Vulnerability penalty on 2.1.0 — must survive so the targeted
          // update lands on 2.0.0 instead of the vulnerable latest.
          '>=2.1.0': { selectorType: 'range', weight: -1000 },
        },
      },
      updateRequested: true,
    }
  )

  expect(resolveResult).toMatchObject({ id: '@acme/private@gh:2.0.0' })
})

test('resolveFromNamedRegistry() suppresses latest when publishedBy holds back the raw tag', async () => {
  // gh-acme-private has 1.0.0 (2024-01-15), 2.0.0 (2024-06-01), 2.1.0 (2024-08-01);
  // dist-tags.latest = 2.1.0. publishedBy 2024-07-01 leaves 2.1.0 immature, so the
  // named-registry path (which shares pickFromSimpleRegistry with JSR) must
  // suppress latest rather than surface a tag the policy would refuse to install.
  interceptGhAcmePrivate()

  const cacheDir = temporaryDirectory()
  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir,
    filterMetadata: true,
    fullMetadata: true,
    registriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    { publishedBy: new Date('2024-07-01T00:00:00.000Z') }
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    id: '@acme/private@gh:2.0.0',
  })
  expect(resolveResult!.latest).toBeUndefined()
})

test('resolveFromNamedRegistry() qualifies the id with the registry alias', async () => {
  interceptGhAcmePrivate()

  const { resolveFromNamedRegistry } = createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
  })

  const resolveResult = await resolveFromNamedRegistry(
    { alias: '@acme/private', bareSpecifier: 'gh:^2.0.0' },
    {}
  )

  expect(resolveResult).toMatchObject({
    resolvedVia: 'named-registry',
    registryName: 'gh',
    id: '@acme/private@gh:2.1.0',
    manifest: {
      name: '@acme/private',
      version: '2.1.0',
    },
  })
})

test('creating the resolver throws when a named registry alias is a reserved specifier prefix', () => {
  expect(() => createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      file: ENTERPRISE_REGISTRY,
    }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME' }))
})

test('creating the resolver throws when a named registry alias is malformed', () => {
  expect(() => createNpmResolver(fetch, () => undefined, {
    storeDir: temporaryDirectory(),
    cacheDir: temporaryDirectory(),
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix({
      'bad alias!': ENTERPRISE_REGISTRY,
    }),
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME' }))
})
