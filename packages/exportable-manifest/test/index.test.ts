/// <reference path="../../../typings/index.d.ts"/>
import exportableManifest, { overridePublishConfig } from '@pnpm/exportable-manifest'
import { PackageManifest, PublishConfig } from '@pnpm/types'

test('the pnpm options are removed', async () => {
  expect(await exportableManifest(process.cwd(), {
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

test('publish lifecycle scripts are removed', async () => {
  expect(await exportableManifest(process.cwd(), {
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
  expect(await exportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
  }, { readmeFile: 'readme content' })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    readme: 'readme content',
  })
})

test('publish config to be overridden', async () => {
  const publishConfig: PublishConfig = {
    main: 'overridden',
    types: 'overridden',
    typesVersions: {
      '*': {
        '*': ['overridden'],
      },
    },
  }
  const publishManifest: PackageManifest = {
    name: 'foo',
    version: '1.0.0',
    main: 'origin',
    types: 'origin',
    typesVersions: {
      '*': {
        '*': ['origin'],
      },
    },
    publishConfig,
  }
  overridePublishConfig(publishManifest)

  Object.keys(publishConfig).forEach((publishConfigKey) => {
    expect(publishManifest[publishConfigKey]).toEqual(publishConfig[publishConfigKey])
  })
})