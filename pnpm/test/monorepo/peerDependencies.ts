import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'

// Covers https://github.com/pnpm/pnpm/issues/6272
test('peer dependency is not unlinked when adding a new dependency', async () => {
  preparePackages([
    {
      name: 'project-1',

      dependencies: {
        '@pnpm.e2e/abc': '1.0.0',
        '@pnpm.e2e/peer-a': 'workspace:*',
      },
    },
    {
      name: '@pnpm.e2e/peer-a',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    autoInstallPeers: false,
  })
  await execPnpm(['install'])
  await execPnpm(['--filter=project-1', 'add', 'is-odd@1.0.0'])

  const lockfile = readYamlFile<LockfileFile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile!.snapshots!)).toContain('@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@@pnpm.e2e+peer-a)')
})
