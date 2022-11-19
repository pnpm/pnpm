import path from 'path'
import { findWorkspacePackagesNoCheck, arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'

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
