import { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import test = require('tape')

// This is supported for compatibility with Yarn's implementation
// see https://github.com/pnpm/pnpm/issues/2648
test('arrayOfWorkspacePackagesToMap() treats private packages with no version as packages with 0.0.0 version', (t) => {
  const privateProject = {
    manifest: {
      name: 'private-pkg',
    },
  }
  t.deepEqual(arrayOfWorkspacePackagesToMap([privateProject]), {
    'private-pkg': {
      '0.0.0': privateProject,
    },
  })
  t.end()
})
