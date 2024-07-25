import path from 'path'
import {
  findWorkspacePackagesNoCheck,
  findWorkspacePackages,
} from '@pnpm/workspace.find-packages'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { logger } from '@pnpm/logger'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

test('findWorkspacePackagesNoCheck() skips engine checks', async () => {
  const fixturePath = path.join(__dirname, '__fixtures__/bad-engine')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspacePackagesNoCheck(fixturePath, {
    patterns: workspaceManifest.packages,
  })
  expect(pkgs.length).toBe(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspacePackages() output warnings for non-root workspace project', async () => {
  const fixturePath = path.join(__dirname, '__fixtures__/warning-for-non-root-project')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspacePackages(fixturePath, {
    patterns: workspaceManifest.packages,
    sharedWorkspaceLockfile: true,
  })
  expect(pkgs.length).toBe(3)
  expect(logger.warn).toHaveBeenCalledTimes(3)
  const fooPath = path.join(fixturePath, 'packages/foo')
  const barPath = path.join(fixturePath, 'packages/bar')
  expect(logger.warn).toHaveBeenNthCalledWith(1, { prefix: barPath, message: `The field "pnpm" was found in ${barPath}/package.json. This will not take effect. You should configure "pnpm" at the root of the workspace instead.` })
  expect(logger.warn).toHaveBeenNthCalledWith(2, { prefix: barPath, message: `The field "resolutions" was found in ${barPath}/package.json. This will not take effect. You should configure "resolutions" at the root of the workspace instead.` })
  expect(logger.warn).toHaveBeenNthCalledWith(3, { prefix: fooPath, message: `The field "pnpm" was found in ${fooPath}/package.json. This will not take effect. You should configure "pnpm" at the root of the workspace instead.` })
})
