import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
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

test('prune --prod removes devDependencies when lockfile is disabled', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  writeYamlFileSync('pnpm-workspace.yaml', { lockfile: false })

  await execPnpm(['install'])
  expect(fs.existsSync('pnpm-lock.yaml')).toBe(false)
  project.has('is-positive')
  project.has('is-negative')

  await execPnpm(['prune', '--prod', '--config.confirmModulesPurge=false'])

  expect(fs.existsSync('pnpm-lock.yaml')).toBe(false)
  project.has('is-positive')
  project.hasNot('is-negative')
})

test('prune removes extraneous direct packages when lockfile is already up to date', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install'])
  project.has('is-positive')

  fs.mkdirSync('node_modules/extraneous-pkg', { recursive: true })
  fs.writeFileSync('node_modules/extraneous-pkg/package.json', JSON.stringify({
    name: 'extraneous-pkg',
    version: '1.0.0',
  }))
  project.has('extraneous-pkg')

  await execPnpm(['prune'])

  project.has('is-positive')
  project.hasNot('extraneous-pkg')
})

test('prune removes extraneous direct packages in a project with no dependencies', async () => {
  const project = prepare({})

  await execPnpm(['install'])

  fs.mkdirSync('node_modules/extraneous-pkg', { recursive: true })
  fs.writeFileSync('node_modules/extraneous-pkg/package.json', JSON.stringify({
    name: 'extraneous-pkg',
    version: '1.0.0',
  }))
  project.has('extraneous-pkg')

  await execPnpm(['prune'])

  project.hasNot('extraneous-pkg')
})

test('prune --filter only prunes the selected workspace project', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        devDependencies: {
          '@pnpm.e2e/hello-world-js-bin': '1.0.0',
        },
      },
    },
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          '@pnpm.e2e/foo': '100.0.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  await execPnpm(['install'])

  fs.mkdirSync('node_modules/extraneous-root', { recursive: true })
  fs.writeFileSync('node_modules/extraneous-root/package.json', JSON.stringify({
    name: 'extraneous-root',
    version: '1.0.0',
  }))
  fs.mkdirSync('packages/pkg-a/node_modules/extraneous-a', { recursive: true })
  fs.writeFileSync('packages/pkg-a/node_modules/extraneous-a/package.json', JSON.stringify({
    name: 'extraneous-a',
    version: '1.0.0',
  }))
  fs.mkdirSync('packages/pkg-b/node_modules/extraneous-b', { recursive: true })
  fs.writeFileSync('packages/pkg-b/node_modules/extraneous-b/package.json', JSON.stringify({
    name: 'extraneous-b',
    version: '1.0.0',
  }))

  await execPnpm(['--filter', 'pkg-a', 'prune', '--prod'])

  projects['pkg-a'].has('is-positive')
  projects['pkg-a'].hasNot('is-negative')
  projects['pkg-a'].hasNot('extraneous-a')

  projects['root'].has('@pnpm.e2e/hello-world-js-bin')
  projects['root'].has('extraneous-root')
  projects['pkg-b'].has('is-positive')
  projects['pkg-b'].has('@pnpm.e2e/foo')
  projects['pkg-b'].has('extraneous-b')
  expect(fs.existsSync('node_modules/.pnpm/@pnpm.e2e+foo@100.0.0')).toBe(true)

  await execPnpm(['-r', 'prune', '--prod'])

  projects['root'].hasNot('@pnpm.e2e/hello-world-js-bin')
  projects['root'].hasNot('extraneous-root')
  projects['pkg-b'].has('is-positive')
  projects['pkg-b'].hasNot('@pnpm.e2e/foo')
  projects['pkg-b'].hasNot('extraneous-b')

  // Filtered prune also works when node_modules/.pnpm/lock.yaml (currentLockfile) is absent.
  fs.rmSync('node_modules/.pnpm/lock.yaml', { force: true })
  fs.mkdirSync('packages/pkg-a/node_modules/extraneous-without-current-lockfile', { recursive: true })
  fs.writeFileSync('packages/pkg-a/node_modules/extraneous-without-current-lockfile/package.json', JSON.stringify({
    name: 'extraneous-without-current-lockfile',
    version: '1.0.0',
  }))

  await execPnpm(['--filter', 'pkg-a', 'prune'])

  projects['pkg-a'].hasNot('extraneous-without-current-lockfile')
})

test('prune in a workspace with sharedWorkspaceLockfile=false prunes all workspace projects', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
      },
    },
    {
      location: 'packages/pkg-a',
      package: {
        name: 'pkg-a',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
    {
      location: 'packages/pkg-b',
      package: {
        name: 'pkg-b',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          '@pnpm.e2e/foo': '100.0.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    sharedWorkspaceLockfile: false,
  })

  await execPnpm(['install'])

  fs.mkdirSync('packages/pkg-a/node_modules/extraneous-a', { recursive: true })
  fs.writeFileSync('packages/pkg-a/node_modules/extraneous-a/package.json', JSON.stringify({
    name: 'extraneous-a',
    version: '1.0.0',
  }))

  await execPnpm(['prune', '--prod'])

  projects['pkg-a'].has('is-positive')
  projects['pkg-a'].hasNot('is-negative')
  projects['pkg-a'].hasNot('extraneous-a')
  projects['pkg-b'].has('is-positive')
  projects['pkg-b'].hasNot('@pnpm.e2e/foo')
})
