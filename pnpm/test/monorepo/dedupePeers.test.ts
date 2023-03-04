import { writeFileSync } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import { createPeersFolderSuffix } from '@pnpm/dependency-path'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'

test('deduplicate packages that have peers, when adding new dependency in a workspace', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-grand-parent-with-c': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  writeFileSync('.npmrc', `dedupe-peer-dependents=true
auto-install-peers=false`, 'utf8')
  await execPnpm(['install'])
  await execPnpm(['--filter=project-2', 'add', '@pnpm.e2e/abc@1.0.0'])

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  const depPaths = Object.keys(lockfile.packages ?? {})
  expect(depPaths.length).toBe(8)
  expect(depPaths).toContain(`/@pnpm.e2e/abc/1.0.0${createPeersFolderSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
  expect(depPaths).toContain(`/@pnpm.e2e/abc-parent-with-ab/1.0.0${createPeersFolderSuffix([{ name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
})
