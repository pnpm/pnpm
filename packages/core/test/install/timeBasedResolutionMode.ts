import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { testDefaults } from '../utils'

test('time-based resolution mode', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/bravo', '@pnpm.e2e/romeo'], await testDefaults({ resolutionMode: 'time-based' }))

  const lockfile = await project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/bravo-dep/1.0.1',
    '/@pnpm.e2e/bravo/1.0.0',
    '/@pnpm.e2e/romeo-dep/1.0.0',
    '/@pnpm.e2e/romeo/1.0.0',
  ])
})

test('the lowest version of a direct dependency is installed when resolution mode is time-based', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  }, await testDefaults({ resolutionMode: 'time-based' }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/foo/100.0.0']).toBeTruthy()
  }

  manifest = await install(manifest, await testDefaults({ resolutionMode: 'time-based', update: true }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/foo/100.1.0']).toBeTruthy()
  }
  expect(manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/foo': '^100.1.0',
  })
})

test('time-based resolution mode should not fail when publishedBy date cannot be calculated', async () => {
  prepareEmpty()
  await install({}, await testDefaults({ resolutionMode: 'time-based' }))
})
