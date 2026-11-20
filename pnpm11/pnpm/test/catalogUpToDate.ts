import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectManifest } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('reverting a catalog entry after updating it is detected as an outdated state (#12418)', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  const manifest: ProjectManifest = {
    name: 'test-catalog-up-to-date',
    version: '0.0.0',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
    },
  }
  const project = prepare(manifest)

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['.'],
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })

  await execPnpm(['install'])
  expect(project.readCurrentLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo'].version).toBe('100.0.0')

  await execPnpm(['update', '@pnpm.e2e/foo', '--latest'])
  expect(project.readCurrentLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo'].version).toBe('100.1.0')

  // Restore the catalog entry to the version it had before the update. A
  // subsequent install must notice that the workspace state is no longer up to
  // date and reinstall the previous version instead of reporting
  // "Already up to date".
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['.'],
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })

  await execPnpm(['install'])
  expect(project.readCurrentLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo'].version).toBe('100.0.0')
})
