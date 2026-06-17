import { expect, jest, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

jest.setTimeout(5 * 60 * 1000)

// A project may have its own pnpm-workspace.yaml that `extends` the workspace
// root. With a dedicated lockfile per project (sharedWorkspaceLockfile: false),
// the project resolves `catalog:` dependencies against its own catalogs merged
// with the inherited ones and records them in its own lockfile.
test('a child workspace extends the root catalog and writes a per-project lockfile', async () => {
  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
  ])

  const projects = preparePackages([
    {
      location: '.',
      package: { name: 'root', private: true },
    },
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        private: true,
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:', // overridden by pkg-a's own catalog
          '@pnpm.e2e/bar': 'catalog:', // defined only in pkg-a's catalog
        },
      },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        private: true,
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:', // inherited from the root catalog
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })
  writeYamlFileSync('packages/pkg-a/pnpm-workspace.yaml', {
    extends: '../..',
    catalog: {
      '@pnpm.e2e/foo': '100.1.0', // takes precedence over the inherited entry
      '@pnpm.e2e/bar': '100.0.0',
    },
  })

  await execPnpm(['install'])

  // pkg-a wins the conflict for foo and keeps its own bar entry.
  const pkgALockfile = projects['pkg-a'].readLockfile()
  expect(pkgALockfile.catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.1.0',
    version: '100.1.0',
  })
  expect(pkgALockfile.catalogs?.default?.['@pnpm.e2e/bar']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })

  // pkg-b inherits foo from the root and does not leak pkg-a's bar entry.
  const pkgBLockfile = projects['pkg-b'].readLockfile()
  expect(pkgBLockfile.catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect(pkgBLockfile.catalogs?.default?.['@pnpm.e2e/bar']).toBeUndefined()

  // Editing a child catalog is detected by the optimistic repeat-install fast
  // path (it must not report "Already up to date").
  writeYamlFileSync('packages/pkg-a/pnpm-workspace.yaml', {
    extends: '../..',
    catalog: {
      '@pnpm.e2e/foo': '100.0.0', // reverted to match the root
      '@pnpm.e2e/bar': '100.0.0',
    },
  })

  await execPnpm(['install'])

  expect(projects['pkg-a'].readLockfile().catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
})

// The `<root>` token lets a package reference the workspace root without
// counting `../` segments. It resolves to the nearest ancestor directory that
// has a pnpm-workspace.yaml.
test('a child workspace extends the root through the <root> token', async () => {
  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
  ])

  const projects = preparePackages([
    {
      location: '.',
      package: { name: 'root', private: true },
    },
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        private: true,
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:', // overridden by pkg-a's own catalog
          '@pnpm.e2e/bar': 'catalog:', // defined only in pkg-a's catalog
        },
      },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        private: true,
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:', // inherited from the root catalog
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })
  writeYamlFileSync('packages/pkg-a/pnpm-workspace.yaml', {
    extends: '<root>',
    catalog: {
      '@pnpm.e2e/foo': '100.1.0', // takes precedence over the inherited entry
      '@pnpm.e2e/bar': '100.0.0',
    },
  })

  await execPnpm(['install'])

  const pkgALockfile = projects['pkg-a'].readLockfile()
  expect(pkgALockfile.catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.1.0',
    version: '100.1.0',
  })
  expect(pkgALockfile.catalogs?.default?.['@pnpm.e2e/bar']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })

  const pkgBLockfile = projects['pkg-b'].readLockfile()
  expect(pkgBLockfile.catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
})

// `extends` is not limited to sharedWorkspaceLockfile: false. With the default
// shared lockfile, a root manifest can extend its packages through a glob, so a
// catalog entry defined in a package manifest becomes available workspace-wide.
test('the root aggregates package catalogs through a glob (shared lockfile)', async () => {
  await Promise.all([
    addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' }),
    addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' }),
  ])

  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        private: true,
        dependencies: {
          '@pnpm.e2e/bar': 'catalog:', // defined in a package manifest, inherited via the glob
        },
      },
    },
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        private: true,
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:', // defined in the root catalog
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    extends: 'packages/*',
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })
  writeYamlFileSync('packages/pkg-a/pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/bar': '100.0.0',
    },
  })

  await execPnpm(['install'])

  const rootLockfile = projects['root'].readLockfile()
  expect(rootLockfile.catalogs?.default?.['@pnpm.e2e/bar']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect(rootLockfile.catalogs?.default?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
})
