/// <reference path="../../../__typings__/index.d.ts"/>
import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { type MakePublishManifestOptions, createExportableManifest } from '@pnpm/exportable-manifest'
import { preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { type ProjectManifest } from '@pnpm/types'
import crossSpawn from 'cross-spawn'
import path from 'path'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

const defaultOpts: MakePublishManifestOptions = {
  catalogs: {},
}

test('the pnpm options are removed', async () => {
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    pnpm: {
      overrides: {
        bar: '1',
      },
    },
  }, defaultOpts)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  })
})

test('the packageManager field is removed', async () => {
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    packageManager: 'pnpm@8.0.0',
  }, defaultOpts)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  })
})

test('publish lifecycle scripts are removed', async () => {
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    scripts: {
      prepublishOnly: 'echo',
      prepack: 'echo',
      prepare: 'echo',
      postpack: 'echo',
      publish: 'echo',
      postpublish: 'echo',
      postinstall: 'echo',
      test: 'echo',
    },
  }, defaultOpts)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    scripts: {
      postinstall: 'echo',
      test: 'echo',
    },
  })
})

test('readme added to published manifest', async () => {
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
  }, { ...defaultOpts, readmeFile: 'readme content' })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    readme: 'readme content',
  })
})

test('workspace deps are replaced', async () => {
  const manifest: ProjectManifest = {
    name: 'workspace-protocol-package',
    version: '1.0.0',

    dependencies: {
      bar: 'workspace:@foo/bar@*',
      baz: 'workspace:baz@^',
      foo: 'workspace:*',
      qux: 'workspace:^',
      quux: 'workspace:',
      waldo: 'workspace:^',
      xerox: 'workspace:../xerox',
      xeroxAlias: 'workspace:../xerox',
      corge: 'workspace:1.0.0',
      grault: 'workspace:^1.0.0',
      garply: 'workspace:plugh@2.0.0',
    },
    peerDependencies: {
      foo: 'workspace:>= || ^3.9.0',
      baz: '^1.0.0 || workspace:>',
      bar: 'workspace:^3.0.0',
      qux: 'workspace:^',
      waldo: 'workspace:^1.x',
    },
  }

  preparePackages([
    manifest,
    {
      name: 'baz',
      version: '1.2.3',
    },
    {
      name: '@foo/bar',
      version: '3.2.1',
    },
    {
      name: 'foo',
      version: '4.5.6',
    },
    {
      name: 'qux',
      version: '1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay',
    },
    {
      name: 'quux',
      version: '7.8.9',
    },
    {
      name: 'waldo',
      version: '1.9.0',
    },
    {
      name: 'xerox',
      version: '4.5.6',
    },
    {
      name: 'corge',
      version: '1.0.0',
    },
    {
      name: 'grault',
      version: '1.0.0',
    },
    {
      name: 'plugh',
      version: '2.0.0',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  crossSpawn.sync(pnpmBin, ['install', '--store-dir=store'])

  process.chdir('workspace-protocol-package')

  expect(await createExportableManifest(process.cwd(), manifest, defaultOpts)).toStrictEqual({
    name: 'workspace-protocol-package',
    version: '1.0.0',
    dependencies: {
      bar: 'npm:@foo/bar@3.2.1',
      baz: '^1.2.3',
      foo: '4.5.6',
      qux: '^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay',
      quux: '7.8.9',
      waldo: '^1.9.0',
      xerox: '4.5.6',
      xeroxAlias: 'npm:xerox@4.5.6',
      corge: '1.0.0',
      grault: '^1.0.0',
      garply: 'npm:plugh@2.0.0',
    },
    peerDependencies: {
      baz: '^1.0.0 || >1.2.3',
      foo: '>=4.5.6 || ^3.9.0',
      bar: '^3.0.0',
      qux: '^1.0.0-alpha-a.b-c-something+build.1-aef.1-its-okay',
      waldo: '^1.x',
    },
  })
})

test('catalog deps are replaced', async () => {
  const manifest: ProjectManifest = {
    name: 'catalog-protocol-package',
    version: '1.0.0',

    dependencies: {
      bar: 'catalog:',
    },
    optionalDependencies: {
      baz: 'catalog:baz',
    },
    peerDependencies: {
      foo: 'catalog:foo',
    },
  }

  preparePackages([manifest])

  const workspaceManifest = {
    packages: ['**', '!store/**'],
    catalog: {
      bar: '^1.2.3',
    },
    catalogs: {
      foo: {
        foo: '^1.2.4',
      },
      baz: {
        baz: '^1.2.5',
      },
    },
  }
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  crossSpawn.sync(pnpmBin, ['install', '--store-dir=store'])

  process.chdir('catalog-protocol-package')

  const catalogs = getCatalogsFromWorkspaceManifest(workspaceManifest)
  expect(await createExportableManifest(process.cwd(), manifest, { catalogs })).toStrictEqual({
    name: 'catalog-protocol-package',
    version: '1.0.0',
    dependencies: {
      bar: '^1.2.3',
    },
    optionalDependencies: {
      baz: '^1.2.5',
    },
    peerDependencies: {
      foo: '^1.2.4',
    },
  })
})

test('jsr deps are replaced', async () => {
  const manifest = {
    name: 'jsr-protocol-manifest',
    version: '0.0.0',
    dependencies: {
      '@foo/bar': 'jsr:^1.0.0',
    },
    optionalDependencies: {
      baz: 'jsr:@foo/baz@3.0',
    },
    peerDependencies: {
      qux: 'jsr:@foo/qux',
    },
  } satisfies ProjectManifest

  preparePackages([manifest])

  process.chdir(manifest.name)

  expect(await createExportableManifest(process.cwd(), manifest, { catalogs: {} })).toStrictEqual({
    name: 'jsr-protocol-manifest',
    version: '0.0.0',
    dependencies: {
      '@foo/bar': 'npm:@jsr/foo__bar@^1.0.0',
    },
    optionalDependencies: {
      baz: 'npm:@jsr/foo__baz@3.0',
    },
    peerDependencies: {
      qux: 'npm:@jsr/foo__qux',
    },
  } as Partial<typeof manifest>)
})

test('checks for name', async () => {
  const location = 'package-to-export'
  const manifest = { version: '0.0.0' } satisfies ProjectManifest

  preparePackages([{
    location,
    package: manifest,
  }])

  process.chdir(location)

  await expect(createExportableManifest(process.cwd(), manifest, { catalogs: {} })).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_REQUIRED_FIELD',
    field: 'name',
  })
})

test('checks for version', async () => {
  const location = 'package-to-export'
  const manifest = { name: 'example' } satisfies ProjectManifest

  preparePackages([{
    location,
    package: manifest,
  }])

  process.chdir(location)

  await expect(createExportableManifest(process.cwd(), manifest, { catalogs: {} })).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_REQUIRED_FIELD',
    field: 'version',
  })
})
