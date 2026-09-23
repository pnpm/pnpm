import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  type PackageManifest,
} from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { streamParser } from '@pnpm/logger'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectId, ProjectRootDir, ReadPackageHook } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { readMetadataMirror, testDefaults } from '../utils/index.js'

test('readPackage, afterAllResolved hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  const afterAllResolved = jest.fn((lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage, afterAllResolved async hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  async function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  const afterAllResolved = jest.fn(async (lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage rewrites the specifier of the project own dependency', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    if (manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep'] != null) {
      manifest.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
    },
  }, testDefaults({
    hooks: {
      readPackage: [readPackageHook],
    },
  }))

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
})

test('readPackage rewrites the specifier of a workspace member own dependency', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    if (manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep'] != null) {
      manifest.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ]
  const mutation = [{ mutation: 'install' as const, rootDir: path.resolve('project-1') as ProjectRootDir }]

  await mutateModules(mutation, testDefaults({
    allProjects,
    hooks: { readPackage: [readPackageHook] },
  }))

  const recorded = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  const readMemberEntry = (): unknown => {
    const lockfile = readYamlFileSync<LockfileObject>(WANTED_LOCKFILE)
    return lockfile.importers['project-1' as ProjectId].dependencies?.['@pnpm.e2e/pkg-with-1-dep']
  }
  expect(readMemberEntry()).toStrictEqual(recorded)

  // The repeat install must compare against the hooked manifest too, or
  // the raw range reads as drift and the entry is rewritten.
  await mutateModules(mutation, testDefaults({
    allProjects,
    hooks: { readPackage: [readPackageHook] },
  }))

  expect(readMemberEntry()).toStrictEqual(recorded)
})

test('readPackage hooks array', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook1 (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '50.0.0'
        break
    }
    return manifest
  }

  function readPackageHook2 (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      readPackage: [readPackageHook1, readPackageHook2],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})

test('a readPackage hook that edits a manifest in place does not affect a later install without the hook', async () => {
  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const readPackageHook: ReadPackageHook = (manifest) => {
    if (manifest.name === '@pnpm.e2e/pkg-with-1-dep') {
      manifest.dependencies!['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }

  // Both installs share one store controller, so they share the resolver's
  // metadata cache and the manifest objects it holds.
  const opts = testDefaults()

  const withHook = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], {
    ...opts,
    hooks: { readPackage: [readPackageHook] },
  })
  expect(Object.keys(withHook.readLockfile().snapshots)).toContain('@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0')

  const withoutHook = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], opts)
  expect(Object.keys(withoutHook.readLockfile().snapshots)).toContain('@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
})

test('a readPackage hook that edits a manifest in place does not change the metadata cache', async () => {
  prepareEmpty()
  const readPackageHook: ReadPackageHook = (manifest) => {
    if (manifest.name === '@pnpm.e2e/pkg-with-1-dep') {
      manifest.dependencies!['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }

  const opts = testDefaults({ hooks: { readPackage: [readPackageHook] } })
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)

  const { meta } = await readMetadataMirror(opts.cacheDir, '@pnpm.e2e/pkg-with-1-dep')
  expect(meta.versions['100.0.0'].dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
  })
})

const SUPERSEDED_WARNING = 'Ignoring "is-positive@3.1.0": "is-positive" is controlled by a package extension, readPackage hook, or override, so its specifier "1.0.0" was used instead.'

test('add keeps the hook-provided specifier when the requested one conflicts with a readPackage hook', async () => {
  const project = prepareEmpty()

  const { result: { updatedManifest }, warnings } = await captureWarnings(async () =>
    addDependenciesToPackage(
      { name: 'my-project', version: '0.0.0' },
      ['is-positive@3.1.0'],
      testDefaults({ hooks: { readPackage: [pinIsPositive] } })
    )
  )

  expect(warnings).toContain(SUPERSEDED_WARNING)
  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({ frozenLockfile: true, hooks: { readPackage: [pinIsPositive] } }))
})

test('add leaves the declared range in package.json when a readPackage hook supersedes the request', async () => {
  const project = prepareEmpty()

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0', dependencies: { 'is-positive': '^3.0.0' } },
    ['is-positive@3.1.0'],
    testDefaults({ hooks: { readPackage: [pinIsPositive] } })
  )

  // The request lost to the hook, so it is not the manifest's new specifier
  // either: only a plain install's outcome is left behind.
  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '^3.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({ frozenLockfile: true, hooks: { readPackage: [pinIsPositive] } }))
})

test('add keeps the hook-provided specifier when an override does not explain the rewrite', async () => {
  const project = prepareEmpty()

  // The override claims the requested specifier but not the hook's output, so
  // the hook's rewrite survives it and the override is not its sole cause.
  const overrides = { 'is-positive@^3': '2.0.0' }

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0' },
    ['is-positive@3.1.0'],
    testDefaults({ hooks: { readPackage: [pinIsPositiveOnceDeclared] }, overrides })
  )

  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({
    frozenLockfile: true,
    hooks: { readPackage: [pinIsPositiveOnceDeclared] },
    overrides,
  }))
})

test('add keeps the hook-provided specifier when a parent override does not explain the rewrite', async () => {
  const project = prepareEmpty()

  // A `parent>child` override is invisible to the per-edge overrider, so the probe has to see what
  // the overrides alone produce to tell this apart from an override that explains the rewrite.
  const overrides = { 'my-project>is-positive@^3': '2.0.0' }

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0' },
    ['is-positive@3.1.0'],
    testDefaults({ hooks: { readPackage: [pinIsPositiveOnceDeclared] }, overrides })
  )

  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({
    frozenLockfile: true,
    hooks: { readPackage: [pinIsPositiveOnceDeclared] },
    overrides,
  }))
})

test('add keeps the requested specifier when no readPackage hook governs the dependency', async () => {
  prepareEmpty()

  const { result: { updatedManifest }, warnings } = await captureWarnings(async () =>
    addDependenciesToPackage(
      { name: 'my-project', version: '0.0.0' },
      ['is-negative@1.0.1'],
      testDefaults({ hooks: { readPackage: [pinIsPositive] } })
    )
  )

  expect(warnings).not.toContainEqual(expect.stringContaining('is controlled by'))
  expect(updatedManifest.dependencies).toStrictEqual({ 'is-negative': '1.0.1' })
})

test('add keeps the requested specifier when a packageExtensions entry only fills the gap', async () => {
  prepareEmpty()

  // A packageExtensions entry only injects what the manifest does not declare,
  // so the added declaration overrides it.
  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0' },
    ['is-positive@3.1.0'],
    testDefaults({
      packageExtensions: {
        'my-project@*': {
          dependencies: { 'is-positive': '1.0.0' },
        },
      },
    })
  )

  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('add keeps the hook-pinned specifier when the manifest already converged to it', async () => {
  prepareEmpty()

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0', dependencies: { 'is-positive': '1.0.0' } },
    ['is-positive@3.1.0'],
    testDefaults({ hooks: { readPackage: [pinIsPositive] } })
  )

  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('add models the real declaration when the dependency lives in another field', async () => {
  prepareEmpty()

  // A default add redeclares the alias in the field it already occupies, so a
  // probe that assumed `dependencies` would let a hook pinning that field slip
  // through.
  function pinDevIsPositive (manifest: PackageManifest): PackageManifest {
    return {
      ...manifest,
      devDependencies: { ...manifest.devDependencies, 'is-positive': '1.0.0' },
    }
  }

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0', devDependencies: { 'is-positive': '1.0.0' } },
    ['is-positive@3.1.0'],
    testDefaults({ hooks: { readPackage: [pinDevIsPositive] } })
  )

  expect(updatedManifest.devDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(updatedManifest.dependencies).toBeUndefined()
})

test('add keeps the hook-provided specifier when a hook reacts to the peer declaration', async () => {
  prepareEmpty()

  // `--save-peer` declares the alias under `peerDependencies` too, so a probe
  // that left that out would never see this hook fire.
  function pinIsPositiveOncePeered (manifest: PackageManifest): PackageManifest {
    return manifest.peerDependencies?.['is-positive'] == null
      ? manifest
      : { ...manifest, devDependencies: { ...manifest.devDependencies, 'is-positive': '1.0.0' } }
  }

  const { updatedManifest } = await addDependenciesToPackage(
    { name: 'my-project', version: '0.0.0' },
    ['is-positive@3.1.0'],
    testDefaults({
      hooks: { readPackage: [pinIsPositiveOncePeered] },
      peer: true,
      targetDependenciesField: 'devDependencies',
    })
  )

  expect(updatedManifest.devDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('add keeps the hook-provided specifier when the hook rewrites only after declaration', async () => {
  const project = prepareEmpty()

  const { result: { updatedManifest }, warnings } = await captureWarnings(async () =>
    addDependenciesToPackage(
      { name: 'my-project', version: '0.0.0' },
      ['is-positive@3.1.0'],
      testDefaults({ hooks: { readPackage: [pinIsPositiveOnceDeclared] } })
    )
  )

  expect(warnings).toContain(SUPERSEDED_WARNING)
  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({ frozenLockfile: true, hooks: { readPackage: [pinIsPositiveOnceDeclared] } }))
})

test('add without a version keeps the hook-provided specifier', async () => {
  const project = prepareEmpty()

  const { result: { updatedManifest }, warnings } = await captureWarnings(async () =>
    addDependenciesToPackage(
      { name: 'my-project', version: '0.0.0' },
      ['is-positive'],
      testDefaults({ hooks: { readPackage: [pinIsPositiveOnceDeclared] } })
    )
  )

  expect(warnings).toContain('Ignoring "is-positive@latest": "is-positive" is controlled by a package extension, readPackage hook, or override, so its specifier "1.0.0" was used instead.')
  expect(updatedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['is-positive']).toMatchObject({ specifier: '1.0.0' })

  await install(updatedManifest, testDefaults({ frozenLockfile: true, hooks: { readPackage: [pinIsPositiveOnceDeclared] } }))
})

test('add skips a dependency a readPackage hook removes', async () => {
  const project = prepareEmpty()

  const { result: { updatedManifest }, warnings } = await captureWarnings(async () =>
    addDependenciesToPackage(
      { name: 'my-project', version: '0.0.0' },
      ['is-positive@3.1.0'],
      testDefaults({ hooks: { readPackage: [dropIsPositive] } })
    )
  )

  expect(warnings).toContain('Skipping "is-positive": a package extension, readPackage hook, or override removes it from the manifest, so it cannot be declared.')
  expect(updatedManifest.dependencies).toBeUndefined()
  expect(project.readLockfile().importers['.'].dependencies).toBeUndefined()

  await install(updatedManifest, testDefaults({ frozenLockfile: true, hooks: { readPackage: [dropIsPositive] } }))
})

function pinIsPositive (manifest: PackageManifest): PackageManifest {
  return manifest.name === 'my-project'
    ? { ...manifest, dependencies: { ...manifest.dependencies, 'is-positive': '1.0.0' } }
    : manifest
}

function pinIsPositiveOnceDeclared (manifest: PackageManifest): PackageManifest {
  return manifest.dependencies?.['is-positive'] == null
    ? manifest
    : { ...manifest, dependencies: { ...manifest.dependencies, 'is-positive': '1.0.0' } }
}

function dropIsPositive (manifest: PackageManifest): PackageManifest {
  if (manifest.dependencies?.['is-positive'] == null) return manifest
  const dependencies = { ...manifest.dependencies }
  delete dependencies['is-positive']
  return { ...manifest, dependencies }
}

async function captureWarnings<T> (run: () => Promise<T>): Promise<{ result: T, warnings: string[] }> {
  const warnings: string[] = []
  const reporter = (log: { level?: string, message?: string }) => {
    if (log.level === 'warn' && log.message != null) warnings.push(log.message)
  }
  streamParser.on('data', reporter as never)
  try {
    return { result: await run(), warnings }
  } finally {
    streamParser.removeListener('data', reporter as never)
  }
}
