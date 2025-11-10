import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils/index.js'

const isOdd011ReleaseDate = new Date(2016, 11, 7 - 2) // 0.1.1 was released at 2016-12-07T07:18:01.205Z
const diff = Date.now() - isOdd011ReleaseDate.getTime()
const minimumReleaseAge = diff / (60 * 1000) // converting to minutes

test('minimumReleaseAge prevents installation of versions that do not meet the required publish date cutoff', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], testDefaults({ minimumReleaseAge }))

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('minimumReleaseAge is ignored for packages in the minimumReleaseAgeExclude array', async () => {
  prepareEmpty()

  const opts = testDefaults({ minimumReleaseAge, minimumReleaseAgeExclude: ['is-odd'] })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge is ignored for packages in the minimumReleaseAgeExclude array, using a pattern', async () => {
  prepareEmpty()

  const opts = testDefaults({ minimumReleaseAge, minimumReleaseAgeExclude: ['is-*'] })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge is ignored for specific exact versions in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  const opts = testDefaults({
    minimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd@0.1.2'],
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  // 0.1.2 is excluded, so it should be installed despite being newer than minimumReleaseAge
  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge applies to versions not in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  const opts = testDefaults({
    minimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd@0.1.0'],
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  // 0.1.2 is NOT excluded (only 0.1.0 is), so minimumReleaseAge applies
  // This should install 0.1.0 which is old enough
  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('throws error when semver range is used in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  await expect(async () => {
    const opts = testDefaults({
      minimumReleaseAge,
      minimumReleaseAgeExclude: ['is-odd@^0.1.1'],
    })
    await addDependenciesToPackage({}, ['is-odd@0.1'], opts)
  }).rejects.toThrow(/Invalid versions union/)
})
