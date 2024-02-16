/// <reference path="../../../__typings__/index.d.ts"/>
import { createExportableManifest } from '@pnpm/exportable-manifest'
import { preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { type ProjectManifest } from '@pnpm/types'
import crossSpawn from 'cross-spawn'
import path from 'path'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

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
  })).toStrictEqual({
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
  })).toStrictEqual({
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
  })).toStrictEqual({
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
  }, { readmeFile: 'readme content' })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    readme: 'readme content',
  })
})

test('workspace deps are replaced', async () => {
  const workspaceProtocolPackageManifest: ProjectManifest = {
    name: 'workspace-protocol-package',
    version: '1.0.0',

    dependencies: {
      bar: 'workspace:@foo/bar@*',
      baz: 'workspace:baz@^',
      foo: 'workspace:*',
    },
  }

  preparePackages([
    workspaceProtocolPackageManifest,
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
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  crossSpawn.sync(pnpmBin, ['install', '--store-dir=store'])

  process.chdir('workspace-protocol-package')

  expect(await createExportableManifest(process.cwd(), workspaceProtocolPackageManifest)).toStrictEqual({
    name: 'workspace-protocol-package',
    version: '1.0.0',
    dependencies: {
      bar: 'npm:@foo/bar@3.2.1',
      baz: '^1.2.3',
      foo: '4.5.6',
    },
  })
})
