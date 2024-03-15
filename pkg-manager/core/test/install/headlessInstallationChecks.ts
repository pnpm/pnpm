import { prepareEmpty } from '@pnpm/prepare'
import { getSystemNodeVersion } from '@pnpm/package-is-installable'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/core'
import { testDefaults } from '../utils'

jest.mock('@pnpm/package-is-installable', () => ({
  ...jest.requireActual('@pnpm/package-is-installable'),
  getSystemNodeVersion: jest.fn(() => process.version),
}))

afterEach(() => {
  jest.mocked(getSystemNodeVersion).mockRestore()
})

test('do not fail if package supports the system Node and engine-strict = true', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    [
      '@pnpm.e2e/for-legacy-node',
    ],
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/for-legacy-node@1.0.0'].engines).toStrictEqual({ node: '0.10' })

  await expect(install(manifest, testDefaults({ engineStrict: true }))).rejects.toThrow()

  jest.mocked(getSystemNodeVersion).mockImplementation(() => '0.10.0')

  await expect(install(manifest, testDefaults({ engineStrict: true }))).resolves.not.toThrow()
})
