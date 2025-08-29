import path from 'path'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { install, remove } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { sync as readYamlFile } from 'read-yaml-file'
import { DEFAULT_OPTS } from '../utils/index.js'

test('remove --filter only changes the specified dependency, when run with link-workspace-packages=false', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1.0.0',
        'is-negative': '1.0.0',
      },
    },
  ])

  const sharedOpts = {
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    sharedWorkspaceLockfile: true,
    linkWorkspacePackages: false,
  }

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    ...sharedOpts,
  })

  await remove.handler({
    ...DEFAULT_OPTS,
    // Only remove is-negative from project-2
    ...await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-2' }]),
    ...sharedOpts,
  }, ['is-negative'])

  // project-1 should be unchanged
  {
    const pkg = await import(path.resolve('project-1/package.json'))
    expect(pkg?.dependencies).toStrictEqual({
      'is-negative': '1.0.0',
    })
  }

  // project-2 has the is-negative dependency removed
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    expect(pkg?.dependencies).toStrictEqual({
      'project-1': '1.0.0',
    })
  }

  // Anything left can still be resolved
  projects['project-1'].has('is-negative')
  projects['project-2'].has('project-1')
  projects['project-2'].hasNot('is-negative')

  // The lockfile agrees with the above
  const lockfile = readYamlFile<LockfileFile>('./pnpm-lock.yaml')

  expect(lockfile.importers?.['project-1'].dependencies?.['is-negative']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })

  expect(lockfile.importers?.['project-2'].dependencies?.['project-1']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })
})

test('remove from within a workspace package dir only affects the specified dependency, when run with link-workspace-packages=false', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1.0.0',
        'is-negative': '1.0.0',
      },
    },
  ])

  const sharedOpts = {
    dir: process.cwd(),
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    sharedWorkspaceLockfile: true,
    linkWorkspacePackages: false,
  }

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    ...sharedOpts,
    recursive: true,
  })

  await remove.handler({
    ...DEFAULT_OPTS,
    ...sharedOpts,
    // In this scenario, remove is invoked from within a workspace directory,
    // non-recursively
    dir: projects['project-2'].dir(),
    recursive: false,
  }, ['is-negative'])

  // project-1 should be unchanged
  {
    const pkg = await import(path.resolve('project-1/package.json'))
    expect(pkg?.dependencies).toStrictEqual({
      'is-negative': '1.0.0',
    })
  }

  // project-2 has the is-negative dependency removed
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    expect(pkg?.dependencies).toStrictEqual({
      'project-1': '1.0.0',
    })
  }

  // Anything left can still be resolved
  projects['project-1'].has('is-negative')
  projects['project-2'].has('project-1')
  projects['project-2'].hasNot('is-negative')

  // The lockfile agrees with the above
  const lockfile = readYamlFile<LockfileFile>('./pnpm-lock.yaml')

  expect(lockfile.importers?.['project-1'].dependencies?.['is-negative']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })

  expect(lockfile.importers?.['project-2'].dependencies?.['project-1']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })
})

test('remove cleans up unused catalogs when cleanupUnusedCatalogs is enabled', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-negative': 'catalog:',
      },
    },
    {
      name: 'project-2', 
      version: '1.0.0',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
  ])

  // Create workspace with catalog entries
  const fs = await import('fs')
  await fs.promises.writeFile('pnpm-workspace.yaml', `
packages:
  - project-1
  - project-2

catalog:
  is-negative: "1.0.0"
  is-positive: "1.0.0"
  unused-pkg: "2.0.0"
`)

  const sharedOpts = {
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    sharedWorkspaceLockfile: true,
    linkWorkspacePackages: false,
    cleanupUnusedCatalogs: true,
  }

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    ...sharedOpts,
  })

  // Remove is-negative dependency, which should cleanup unused-pkg from catalog
  await remove.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }]),
    ...sharedOpts,
  }, ['is-negative'])

  // Check that unused catalog entry was cleaned up
  const workspaceManifest = readYamlFile('./pnpm-workspace.yaml')
  expect(workspaceManifest.catalog).toStrictEqual({
    'is-positive': '1.0.0',
    // 'unused-pkg' should be removed since it's not referenced by any project
  })
})
