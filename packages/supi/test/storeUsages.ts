import assertStore from '@pnpm/assert-store'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  storeAdd,
  storeUsages
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('find usages for single package in store and in a project', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // Install deps
  await addDependenciesToPackage({}, ['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsagesBySelectors = await storeUsages(['is-negative'], await testDefaults())

  // Assert
  t.equal(packageUsagesBySelectors['is-negative'].length, 1, 'number of items in response should be 1')

  const packageUsages = packageUsagesBySelectors['is-negative'][0]

  t.equal(packageUsages.packageId, `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`, 'correct packageId found')
  t.equal(packageUsages.usages.length, 1, 'there should only be 1 usage be found')
})

test('find usages for single package in store (by version) and in a project', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // Install deps
  await addDependenciesToPackage({}, ['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsagesBySelectors = await storeUsages(['is-negative@2.1.0'], await testDefaults())

  // Assert
  t.equal(packageUsagesBySelectors['is-negative@2.1.0'].length, 1, 'number of items in response should be 1')

  const packageUsages = packageUsagesBySelectors['is-negative@2.1.0'][0]

  t.equal(packageUsages.packageId, `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`, 'correct packageId found')
  t.equal(packageUsages.usages.length, 1, 'there should only be 1 usage be found')
})

test('find usages for package not in store', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults()
  const store = assertStore(t, opts.storeDir)

  // Find usages
  await store.storeHasNot('should-not-exist-uhsalzkj')
  const packageUsagesBySelectors = await storeUsages(['should-not-exist-uhsalzkj'], opts)

  t.deepEqual(packageUsagesBySelectors, {
    'should-not-exist-uhsalzkj': [],
  }, 'search should not find anything')
})

test('find usages of packages in store (multiple queries)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // Install deps
  let manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0'], await testDefaults())
  await project.storeHas('is-negative', '2.1.0')
  await addDependenciesToPackage(manifest, ['is-odd@3.0.0'], await testDefaults())
  await project.storeHas('is-odd', '3.0.0')

  // Find usages
  const packageUsagesBySelectors = await storeUsages(['is-negative', 'is-odd'], await testDefaults())

  // Assert
  t.equal(Object.keys(packageUsagesBySelectors).length, 2, 'correnct number of items in response')

  {
    t.equal(packageUsagesBySelectors['is-negative'].length, 1, 'one "is-negative" package found in store')

    const packageUsages = packageUsagesBySelectors['is-negative'][0]

    t.equal(packageUsages.packageId, `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`, 'correct packageId found')
    t.equal(packageUsages.usages.length, 1, 'there should only be 1 usage be found')
  }

  {
    t.equal(packageUsagesBySelectors['is-odd'].length, 1, 'one "is-odd" package found in store')

    const packageUsages = packageUsagesBySelectors['is-odd'][0]

    t.equal(packageUsages.packageId, `localhost+${REGISTRY_MOCK_PORT}/is-odd/3.0.0`, 'correct packageId found')
    t.equal(packageUsages.usages.length, 1, 'there should only be 1 usage be found')
  }
})

test('find usages for package in store but not in any projects', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults()
  const registries = opts.registries || {
    default: 'null'
  }
  const store = assertStore(t, opts.storeDir)

  // Add dependency directly to store (not to the project)
  await storeAdd(['is-negative'], {
    registries,
    tag: '2.1.0',
    ...opts
  })
  await store.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsagesBySelectors = await storeUsages(['is-negative'], opts)

  t.deepEqual(
    packageUsagesBySelectors,
    {
      'is-negative': [
        {
          packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
          usages: [],
        },
      ],
    },
  )
})

test('find usages for multiple packages in store but not in any projects', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults()
  const registries = opts.registries || {
    default: 'null'
  }
  const store = assertStore(t, opts.storeDir)

  // Add dependencies directly to store (not to the project). Note we add different versions of the same package
  await storeAdd(['is-negative'], {
    registries,
    tag: '2.0.0',
    ...opts
  })
  await store.storeHas('is-negative', '2.0.0')
  await storeAdd(['is-negative'], {
    registries,
    tag: '2.1.0',
    ...opts
  })
  await store.storeHas('is-negative', '2.1.0')

  // Find usages
  const packageUsagesBySelectors = await storeUsages(['is-negative'], opts)

  t.deepEqual(
    packageUsagesBySelectors,
    {
      'is-negative': [
        {
          packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.0.0`,
          usages: [],
        },
        {
          packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
          usages: [],
        },
      ],
    },
  )
})
