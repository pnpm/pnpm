import prepare from '@pnpm/prepare'
import { PackageUsage } from '@pnpm/store-controller-types'
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
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative'], await testDefaults())

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'is-negative', 'query does not match')
  t.true(packageUsage.foundInStore, 'query not found in store')
  t.equal(packageUsage.packages.length, 1, 'there should only be 1 package returned from the query')

  const packageFound = packageUsage.packages[0]

  t.ok(packageFound.id, 'there should be a package id')
  t.equal(packageFound.usages.length, 0, 'package should not be used by any projects')
})
