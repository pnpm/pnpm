import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
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
