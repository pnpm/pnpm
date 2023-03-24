import { promises as fs } from 'fs'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { Lockfile } from '@pnpm/lockfile-types'
import { preparePackages } from '@pnpm/prepare'
import readYamlFile from 'read-yaml-file'
import writeYamlFile from 'write-yaml-file'
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

  await fs.writeFile('.npmrc', 'auto-install-peers=false', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])
  await execPnpm(['--filter=project-1', 'add', 'is-odd@1.0.0'])

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile!.packages!)).toContain('/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@@pnpm.e2e+peer-a)')
})
