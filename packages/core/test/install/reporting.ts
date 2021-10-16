import { DeprecationLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
} from '@pnpm/core'
import * as sinon from 'sinon'
import { testDefaults } from '../utils'

// TODO: use a smaller package for testing deprecation
test('reports warning when installing deprecated packages', async () => {
  const project = prepareEmpty()

  let manifest
  {
    const reporter = sinon.spy()

    manifest = await addDependenciesToPackage({}, ['express@0.14.1'], await testDefaults({ fastUnpack: false, reporter }))

    expect(reporter.calledWithMatch({
      deprecated: 'express 0.x series is deprecated',
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: `localhost+${REGISTRY_MOCK_PORT}/express/0.14.1`,
    } as DeprecationLog)).toBeTruthy()
  }

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/express/0.14.1'].deprecated).toBe('express 0.x series is deprecated')

  {
    const reporter = sinon.spy()

    await addDependenciesToPackage(manifest, ['express@4.16.3'], await testDefaults({ fastUnpack: false, reporter }))

    expect(reporter.calledWithMatch({
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: `localhost+${REGISTRY_MOCK_PORT}/express/4.16.3`,
    } as DeprecationLog)).toBeFalsy()
  }
})
