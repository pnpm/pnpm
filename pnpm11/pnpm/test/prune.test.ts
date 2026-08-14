import { test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('prune --prod in a workspace root keeps the links to workspace packages that are production dependencies', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
      },
    },
    {
      location: 'apps/app',
      package: {
        name: 'app',
        version: '1.0.0',
        dependencies: {
          '@scope/lib': 'workspace:*',
          'is-positive': '1.0.0',
        },
        devDependencies: {
          '@scope/tool': 'workspace:*',
          'is-negative': '1.0.0',
        },
      },
    },
    {
      location: 'packages/lib',
      package: {
        name: '@scope/lib',
        version: '1.0.0',
      },
    },
    {
      location: 'packages/tool',
      package: {
        name: '@scope/tool',
        version: '1.0.0',
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['apps/*', 'packages/*'] })

  await execPnpm(['install'])

  projects.app.has('@scope/lib')
  projects.app.has('@scope/tool')
  projects.app.has('is-positive')
  projects.app.has('is-negative')

  await execPnpm(['prune', '--prod', '--config.confirmModulesPurge=false'])

  projects.app.has('@scope/lib')
  projects.app.has('is-positive')
  projects.app.hasNot('@scope/tool')
  projects.app.hasNot('is-negative')
})
