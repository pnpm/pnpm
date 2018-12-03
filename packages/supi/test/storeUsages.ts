import prepare from '@pnpm/prepare'
import { FindPackageUsagesResponse } from '@pnpm/store-controller-types'
import {
  addDependenciesToPackage,
  storeUsages,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)

test('find usages for newly installed package', async (t: tape.Test) => {
  const project = prepare(t)

  // Install deps
  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults({ save: true }))
  await project.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsagesResponses: FindPackageUsagesResponse[]
    = await storeUsages(['is-negative'], await testDefaults())

  // Assert
  t.equal(packageUsagesResponses.length, 1, 'number of items in response should be 1')

  const packageUsageResponse = packageUsagesResponses[0]

  t.equal(packageUsageResponse.dependency.alias, 'is-negative', 'query does not match')
  t.true(packageUsageResponse.foundInStore, 'query not found in store')
  t.equal(packageUsageResponse.packages.length, 1, 'there should only be 1 package returned from the query')

  const packageUsed = packageUsageResponse.packages[0]

  t.ok(packageUsed.id, 'there should be a package id')
  t.equal(packageUsed.usages.length, 0, 'package should not be used by any projects')
})
