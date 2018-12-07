import prepare from '@pnpm/prepare'
import { PackageUsage } from '@pnpm/store-controller-types'
import {
  addDependenciesToPackage,
  storeAdd,
  storeUsages
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)

test('find usages for single package in store and in a project', async (t: tape.Test) => {
  const project = prepare(t)

  // Install deps
  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative'], await testDefaults())

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'is-negative', 'query name should match')
  t.equal(packageUsage.dependency.pref, 'latest', 'query version should be latest (not specified)')
  t.true(packageUsage.foundInStore, 'query should be found in store')
  t.equal(packageUsage.packages.length, 1, 'there should only be 1 package returned for the query')

  const packageFound = packageUsage.packages[0]

  t.ok(packageFound.id, 'there should be a package id')
  t.true(packageFound.id.includes('is-negative'), 'package name should be correct')
  t.true(packageFound.id.includes('2.1.0'), 'package version should be correct')
  t.equal(packageFound.usages.length, 1, 'package should be used by only 1 project')

  // For debugging
  const location = packageFound.usages[0]
  console.log('pnpm project location: ' + location)
})

test('find usages for single package in store (by version) and in a project', async (t: tape.Test) => {
  const project = prepare(t)

  // Install deps
  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative@2.1.0'], await testDefaults())

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'is-negative', 'query name should match')
  t.equal(packageUsage.dependency.pref, '2.1.0', 'query version should be latest (not specified)')
  t.true(packageUsage.foundInStore, 'query should be found in store')
  t.equal(packageUsage.packages.length, 1, 'there should only be 1 package returned for the query')

  const packageFound = packageUsage.packages[0]

  t.ok(packageFound.id, 'there should be a package id')
  t.true(packageFound.id.includes('is-negative'), 'package name should be correct')
  t.true(packageFound.id.includes('2.1.0'), 'package version should be correct')
  t.equal(packageFound.usages.length, 1, 'package should be used by only 1 project')

  // For debugging
  const location = packageFound.usages[0]
  console.log('pnpm project location: ' + location)
})

test('find usages for package not in store', async (t: tape.Test) => {
  const project = prepare(t)

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['should-not-exist-uhsalzkj'], await testDefaults())

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'should-not-exist-uhsalzkj', 'query should match')
  t.equal(packageUsage.dependency.pref, 'latest', 'query version should be latest (not specified)')
  t.false(packageUsage.foundInStore, 'query should not be in store')
  t.equal(packageUsage.packages.length, 0, 'there should no packages returned for the query')
})

test('find usages of packages in store (multiple queries)', async (t: tape.Test) => {
  const project = prepare(t)

  const packages = ['is-negative', 'is-odd']

  // Install deps
  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')
  await addDependenciesToPackage(['is-odd@3.0.0'], await testDefaults())
  await project.storeHas('is-odd', '3.0.0')

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative', 'is-odd'], await testDefaults())

  // Assert
  t.equal(packageUsages.length, 2, 'number of items in response should be 1')

  packageUsages.forEach(packageUsage => {
    t.ok(packageUsage.dependency.alias, 'query name should exist')
    t.true(packages.indexOf(packageUsage.dependency.alias || 'null') > -1, 'query name should match')
    t.equal(packageUsage.dependency.pref, 'latest', 'query version should be latest (not specified)')
    t.true(packageUsage.foundInStore, 'query should be found in store')
    t.equal(packageUsage.packages.length, 1, 'there should only be 1 package returned for the query')

    const packageFound = packageUsage.packages[0]

    t.ok(packageFound.id, 'there should be a package id')
    console.log('package id: ' + packageFound.id)
    t.equal(packageFound.usages.length, 1, 'package should be used by only 1 project')

    // For debugging
    const location = packageFound.usages[0]
    console.log('pnpm project location: ' + location)
  })
})

test('find usages for package in store but not in any projects', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults()
  const registries = opts.registries || {
    default: 'null'
  }

  // Add dependency directly to store (not to the project)
  await storeAdd(['is-negative'], {
    registry: registries.default,
    tag: '2.1.0',
    ...opts
  })

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative'], opts)

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'is-negative', 'query name should match')
  t.equal(packageUsage.dependency.pref, 'latest', 'query version should be latest (not specified)')
  t.true(packageUsage.foundInStore, 'query should be found in store')
  t.equal(packageUsage.packages.length, 1, 'there should only be 1 package returned for the query')

  const packageFound = packageUsage.packages[0]
  console.log(packageFound)

  t.ok(packageFound.id, 'there should be a package id')
  t.true(packageFound.id.includes('is-negative'), 'package name should be correct')
  t.true(packageFound.id.includes('2.1.0'), 'package version should be correct')
  t.equal(packageFound.usages.length, 0, 'package should be used by no projects')
})

test('find usages for multiple packages in store but not in any projects', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults()
  const registries = opts.registries || {
    default: 'null'
  }

  // Add dependencies directly to store (not to the project). Note we add different versions of the same package
  await storeAdd(['is-negative'], {
    registry: registries.default,
    tag: '2.0.0',
    ...opts
  })
  await storeAdd(['is-negative'], {
    registry: registries.default,
    tag: '2.1.0',
    ...opts
  })

  // Find usages
  const packageUsages: PackageUsage[] = await storeUsages(['is-negative'], opts)

  // Assert
  t.equal(packageUsages.length, 1, 'number of items in response should be 1')

  const packageUsage = packageUsages[0]

  t.equal(packageUsage.dependency.alias, 'is-negative', 'query name should match')
  t.equal(packageUsage.dependency.pref, 'latest', 'query version should be latest (not specified)')
  t.true(packageUsage.foundInStore, 'query should be found in store')
  t.equal(packageUsage.packages.length, 2, 'there should be 2 packages returned for the query')

  const seenPackageIds = new Set<string>()

  packageUsage.packages.forEach(packageFound => {
    console.log(packageFound)

    // Ensure id is correct
    t.ok(packageFound.id, 'there should be a package id')
    t.false(seenPackageIds.has(packageFound.id), 'this package id should be unique')
    seenPackageIds.add(packageFound.id)

    t.true(packageFound.id.includes('is-negative'), 'package name should be correct')
    t.true(packageFound.id.includes('2.1.0') || packageFound.id.includes('2.0.0'),
      'package version should be correct')

    // Ensure no usages
    t.equal(packageFound.usages.length, 0, 'package should be used by no projects')
  })
})
