import { PackageManifest, PublishConfig } from '@pnpm/types'
import { overridePublishConfig } from '../lib/overridePublishConfig'

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
