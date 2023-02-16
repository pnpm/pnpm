import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as writeJsonFile } from 'write-json-file'
import { testDefaults } from '../utils'

test('remove broken cache', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'], await testDefaults({ cacheDir }))

  const metadataCachePath = path.resolve(cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.json`)
  const metaCache = loadJsonFile<any>(metadataCachePath) // eslint-disable-line @typescript-eslint/no-explicit-any
  metaCache.versions['100.0.0'].dist.integrity = 'sha512-7KxauUdBmSdWnmpaGFg+ppNjKF8uNLry8LyzjauQDOVONfFLNKrKvQOxZ/VuTIcS/gge/YNahf5RIIQWTSarlg=='
  metaCache.cachedAt = Date.now() - 1000 * 60 * 60 * 24 * 30
  writeJsonFile(metadataCachePath, metaCache)

  await rimraf('node_modules')
  await rimraf('pnpm-lock.yaml')

  await install(manifest, await testDefaults({ cacheDir }))
})
