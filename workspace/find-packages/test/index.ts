import path from 'path'
import {
  findWorkspacePackagesNoCheck,
  arrayOfWorkspacePackagesToMap,
  findWorkspacePackages,
} from '@pnpm/workspace.find-packages'
import { logger } from '@pnpm/logger'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

// This is supported for compatibility with Yarn's implementation
// see https://github.com/pnpm/pnpm/issues/2648
test('arrayOfWorkspacePackagesToMap() treats private packages with no version as packages with 0.0.0 version', () => {
  const privateProject = {
    manifest: {
      name: 'private-pkg',
    },
  }
  expect(arrayOfWorkspacePackagesToMap([privateProject])).toStrictEqual({
    'private-pkg': {
      '0.0.0': privateProject,
    },
  })
})

test('findWorkspacePackagesNoCheck() skips engine checks', async () => {
  const pkgs = await findWorkspacePackagesNoCheck(path.join(__dirname, '__fixtures__/bad-engine'))
  expect(pkgs.length).toBe(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspacePackages() output warnings for non-root workspace project', async () => {
  const fixturePath = path.join(__dirname, '__fixtures__/warning-for-non-root-project')

  const pkgs = await findWorkspacePackages(fixturePath)
  expect(pkgs.length).toBe(3)
  expect(logger.warn).toBeCalledTimes(3)
  const fooPath = path.join(fixturePath, 'packages/foo')
  const barPath = path.join(fixturePath, 'packages/bar')
  expect(logger.warn).toHaveBeenNthCalledWith(1, { prefix: barPath, message: `The field "pnpm" was found in ${barPath}/package.json. This will not take effect. You should configure "pnpm" at the root of the workspace instead.` })
  expect(logger.warn).toHaveBeenNthCalledWith(2, { prefix: barPath, message: `The field "resolutions" was found in ${barPath}/package.json. This will not take effect. You should configure "resolutions" at the root of the workspace instead.` })
  expect(logger.warn).toHaveBeenNthCalledWith(3, { prefix: fooPath, message: `The field "pnpm" was found in ${fooPath}/package.json. This will not take effect. You should configure "pnpm" at the root of the workspace instead.` })
})
