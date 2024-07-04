import { type PnpmError } from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, mutateModulesInSingleProject } from '@pnpm/core'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectRootDir } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('fail if none of the available resolvers support a version spec', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await mutateModulesInSingleProject({
      manifest: {
        dependencies: {
          '@types/plotly.js': '1.44.29',
        },
      },
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults())
    throw new Error('should have failed')
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER')
  expect(err.prefix).toBe(process.cwd())
  expect(err.pkgsStack).toStrictEqual(
    [
      {
        id: '@types/plotly.js@1.44.29',
        name: '@types/plotly.js',
        version: '1.44.29',
      },
    ]
  )
})

test('fail if a package cannot be fetched', async () => {
  prepareEmpty()
  /* eslint-disable @typescript-eslint/no-explicit-any */
  nock(`http://localhost:${REGISTRY_MOCK_PORT}/`)
    .get('/@pnpm.e2e%2Fpkg-with-1-dep') // cspell:disable-line
    .reply(200, loadJsonFile.sync<any>(f.find('pkg-with-1-dep.json')))
  nock(`http://localhost:${REGISTRY_MOCK_PORT}/`)
    .get('/@pnpm.e2e%2Fdep-of-pkg-with-1-dep') // cspell:disable-line
    .reply(200, loadJsonFile.sync<any>(f.find('dep-of-pkg-with-1-dep.json')))
  /* eslint-enable @typescript-eslint/no-explicit-any */
  nock(`http://localhost:${REGISTRY_MOCK_PORT}/`)
    .get('/@pnpm.e2e/pkg-with-1-dep/-/@pnpm.e2e/pkg-with-1-dep-100.0.0.tgz')
    .replyWithFile(200, f.find('pkg-with-1-dep-100.0.0.tgz'))
  nock(`http://localhost:${REGISTRY_MOCK_PORT}/`)
    .get('/@pnpm.e2e/dep-of-pkg-with-1-dep/-/@pnpm.e2e/dep-of-pkg-with-1-dep-100.1.0.tgz')
    .reply(403)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], testDefaults({}, {}, { retry: { retries: 0 } }))
    throw new Error('should have failed')
  } catch (_err: any) { // eslint-disable-line
    nock.restore()
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_FETCH_403')
  expect(err.prefix).toBe(process.cwd())
  expect(err.pkgsStack).toStrictEqual(
    [
      {
        id: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
        name: '@pnpm.e2e/pkg-with-1-dep',
        version: '100.0.0',
      },
    ]
  )
})
