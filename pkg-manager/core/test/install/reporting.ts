import { DeprecationLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
} from '@pnpm/core'
import { testDefaults } from '../utils'

// TODO: use a smaller package for testing deprecation
test('reports warning when installing deprecated packages', async () => {
  const project = prepareEmpty()
  const reporter = jest.fn()

  const manifest = await addDependenciesToPackage({}, ['express@0.14.1'], await testDefaults({ fastUnpack: false, reporter }))

  expect(reporter).toBeCalledWith(expect.objectContaining({
    deprecated: 'express 0.x series is deprecated',
    level: 'debug',
    name: 'pnpm:deprecation',
    pkgId: `localhost+${REGISTRY_MOCK_PORT}/express/0.14.1`,
  } as DeprecationLog))

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/express/0.14.1'].deprecated).toBe('express 0.x series is deprecated')

  reporter.mockReset()

  await addDependenciesToPackage(manifest, ['express@4.16.3'], await testDefaults({ fastUnpack: false, reporter }))

  expect(reporter).not.toBeCalledWith(expect.objectContaining({
    level: 'debug',
    name: 'pnpm:deprecation',
  } as DeprecationLog))
})

test('doesn\'t report a warning when the deprecated package is allowed', async () => {
  prepareEmpty()
  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['express@0.14.1'], await testDefaults({
    allowedDeprecatedVersions: {
      express: '0.14.1',
    },
    reporter,
  }))

  expect(reporter).not.toBeCalledWith(expect.objectContaining({
    level: 'debug',
    name: 'pnpm:deprecation',
  } as DeprecationLog))
})
