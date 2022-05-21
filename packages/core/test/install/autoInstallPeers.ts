import { addDependenciesToPackage } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from '../utils'

test('auto install peer dependencies', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['abc'], await testDefaults({ autoInstallPeers: true }))
  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/abc/1.0.0_czpb4cfd67t7o7o3k4vnbzkwma',
    '/dep-of-pkg-with-1-dep/100.0.0',
    '/peer-a/1.0.0',
    '/peer-b/1.0.0',
    '/peer-c/1.0.0',
  ])
})
