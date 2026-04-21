import { expect, test } from '@jest/globals'
import {
  applyRuntimeOnFailOverride,
  convertEnginesRuntimeToDependencies,
} from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'

test('convertEnginesRuntimeToDependencies() skips runtime entries without a version', () => {
  const manifest: ProjectManifest = {
    devEngines: {
      runtime: {
        name: 'node',
        onFail: 'download',
      },
    },
  }

  convertEnginesRuntimeToDependencies(manifest, 'devEngines', 'devDependencies')

  expect(manifest.devDependencies).toBeUndefined()
})

test('applyRuntimeOnFailOverride(download) skips runtime entries without a version', () => {
  const manifest: ProjectManifest = {
    devEngines: {
      runtime: {
        name: 'node',
      },
    },
  }

  applyRuntimeOnFailOverride(manifest, 'download')

  expect(manifest.devEngines?.runtime).toMatchObject({ name: 'node', onFail: 'download' })
  expect(manifest.devDependencies).toBeUndefined()
})
