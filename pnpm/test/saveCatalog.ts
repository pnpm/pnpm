import { expect, test } from '@jest/globals'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import type { ProjectManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('--save-catalog adds catalogs to the manifest of a single package workspace', async () => {
  const manifest: ProjectManifest = {
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
    dependencies: {
      '@pnpm.e2e/bar': 'catalog:',
    },
  }

  prepare(manifest)

  writeYamlFileSync('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
    },
  }))

  await execPnpm(['add', '--save-catalog', '@pnpm.e2e/foo'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
      '@pnpm.e2e/foo@100.1.0': expect.anything(),
    },
  }))
  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
      '@pnpm.e2e/foo': '^100.1.0',
    },
  })
  expect(loadJsonFileSync('package.json')).toStrictEqual({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
})

test('--save-catalog adds catalogs to the manifest of a shared lockfile workspace', async () => {
  const manifests: ProjectManifest[] = [
    {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        '@pnpm.e2e/bar': 'catalog:',
      },
    },
    {
      name: 'project-1',
      version: '0.0.0',
    },
  ]

  preparePackages(manifests)

  writeYamlFileSync('pnpm-workspace.yaml', {
    sharedWorkspaceLockfile: true,
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
    packages: ['project-0', 'project-1'],
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      'project-0': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
      'project-1': {},
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
    },
  }))

  await execPnpm(['--filter=project-1', 'add', '--save-catalog', '@pnpm.e2e/foo'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      'project-0': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
      'project-1': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
      '@pnpm.e2e/foo@100.1.0': expect.anything(),
    },
  }))
  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
      '@pnpm.e2e/foo': '^100.1.0',
    },
    packages: ['project-0', 'project-1'],
    sharedWorkspaceLockfile: true,
  })
  expect(loadJsonFileSync('project-1/package.json')).toStrictEqual({
    ...manifests[1],
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
})

test('--save-catalog adds catalogs to the manifest of a multi-lockfile workspace', async () => {
  const manifests: ProjectManifest[] = [
    {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        '@pnpm.e2e/bar': 'catalog:',
      },
    },
    {
      name: 'project-1',
      version: '0.0.0',
    },
  ]

  preparePackages(manifests)

  writeYamlFileSync('pnpm-workspace.yaml', {
    sharedWorkspaceLockfile: false,
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
    packages: ['project-0', 'project-1'],
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  {
    await execPnpm(['install'])

    const lockfile0: LockfileFile = readYamlFileSync('project-0/pnpm-lock.yaml')
    expect(lockfile0.catalogs).toStrictEqual({
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    } as LockfileFile['catalogs'])
    expect(lockfile0.importers).toStrictEqual({
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    } as LockfileFile['importers'])

    const lockfile1: LockfileFile = readYamlFileSync('project-1/pnpm-lock.yaml')
    expect(lockfile1.catalogs).toBeUndefined()
    expect(lockfile1.importers).toStrictEqual({
      '.': {},
    } as LockfileFile['importers'])
  }

  {
    await execPnpm(['--filter=project-1', 'add', '--save-catalog', '@pnpm.e2e/foo'])

    const lockfile0: LockfileFile = readYamlFileSync('project-0/pnpm-lock.yaml')
    expect(lockfile0.catalogs).toStrictEqual({
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    } as LockfileFile['catalogs'])
    expect(lockfile0.importers).toStrictEqual({
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    } as LockfileFile['importers'])

    const lockfile1: LockfileFile = readYamlFileSync('project-1/pnpm-lock.yaml')
    expect(lockfile1.catalogs).toStrictEqual({
      default: {
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    } as LockfileFile['catalogs'])
    expect(lockfile1.importers).toStrictEqual({
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    } as LockfileFile['importers'])

    expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
      catalog: {
        '@pnpm.e2e/bar': '^100.1.0',
        '@pnpm.e2e/foo': '^100.1.0',
      },
      packages: ['project-0', 'project-1'],
      sharedWorkspaceLockfile: false,
    })

    expect(loadJsonFileSync('project-1/package.json')).toStrictEqual({
      ...manifests[1],
      dependencies: {
        ...manifests[1].dependencies,
        '@pnpm.e2e/foo': 'catalog:',
      },
    })
  }
})

test('--save-catalog does not add local workspace dependency as a catalog', async () => {
  const manifests: ProjectManifest[] = [
    {
      name: 'project-0',
      version: '0.0.0',
    },
    {
      name: 'project-1',
      version: '0.0.0',
    },
  ]

  preparePackages(manifests)

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['project-0', 'project-1'],
  })

  {
    await execPnpm(['install'])

    const lockfile: LockfileFile = readYamlFileSync('pnpm-lock.yaml')
    expect(lockfile.catalogs).toBeUndefined()
    expect(lockfile.importers).toStrictEqual({
      'project-0': {},
      'project-1': {},
    })
  }

  {
    await execPnpm(['--filter=project-1', 'add', '--save-catalog', 'project-0@workspace:*'])

    const lockfile: LockfileFile = readYamlFileSync('pnpm-lock.yaml')
    expect(lockfile.catalogs).toBeUndefined()
    expect(lockfile.importers).toStrictEqual({
      'project-0': {},
      'project-1': {
        dependencies: {
          'project-0': {
            specifier: 'workspace:*',
            version: 'link:../project-0',
          },
        },
      },
    })

    expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
      packages: ['project-0', 'project-1'],
    })

    expect(loadJsonFileSync('project-1/package.json')).toStrictEqual({
      ...manifests[1],
      dependencies: {
        'project-0': 'workspace:*',
      },
    })
  }
})

test('--save-catalog does not affect new dependencies from package.json', async () => {
  const manifest: ProjectManifest = {
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
    dependencies: {
      '@pnpm.e2e/pkg-a': 'catalog:',
    },
  }

  const project = prepare(manifest)

  writeYamlFileSync('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/pkg-a': '1.0.0',
    },
  })

  // initialize the lockfile
  await execPnpm(['install'])
  expect(project.readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/pkg-a': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/pkg-a': {
            specifier: 'catalog:',
            version: '1.0.0',
          },
        },
      },
    },
  }))

  // add a new dependency to package.json by editing it
  project.writePackageJson({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/pkg-b': '*',
    },
  })

  // add a new dependency by running `pnpm add --save-catalog`
  await execPnpm(['add', '--save-catalog', '@pnpm.e2e/pkg-c'])

  const lockfile = project.readLockfile()
  expect(lockfile.catalogs).toStrictEqual({
    default: {
      '@pnpm.e2e/pkg-a': {
        specifier: '1.0.0',
        version: '1.0.0',
      },
      '@pnpm.e2e/pkg-c': {
        specifier: '^1.0.0',
        version: '1.0.0',
      },
    },
  } as LockfileFile['catalogs'])
  expect(lockfile.catalogs.default).not.toHaveProperty(['@pnpm.e2e/pkg-b'])
  expect(lockfile.importers).toStrictEqual({
    '.': {
      dependencies: {
        '@pnpm.e2e/pkg-a': {
          specifier: 'catalog:',
          version: '1.0.0',
        },
        '@pnpm.e2e/pkg-b': {
          specifier: '*', // unaffected by `pnpm add --save-catalog`
          version: '1.0.0',
        },
        '@pnpm.e2e/pkg-c': {
          specifier: 'catalog:', // created by `pnpm add --save-catalog`
          version: '1.0.0',
        },
      },
    },
  } as LockfileFile['importers'])

  expect(loadJsonFileSync('package.json')).toStrictEqual({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/pkg-b': '*', // unaffected by `pnpm add --save-catalog`
      '@pnpm.e2e/pkg-c': 'catalog:', // created by `pnpm add --save-catalog`
    },
  })
})

test('--save-catalog does not overwrite existing catalogs', async () => {
  const manifests: ProjectManifest[] = [
    {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        '@pnpm.e2e/bar': 'catalog:',
      },
    },
    {
      name: 'project-1',
      version: '0.0.0',
    },
  ]

  preparePackages(manifests)

  writeYamlFileSync('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/bar': '=100.0.0', // intentionally outdated
    },
    packages: ['project-0', 'project-1'],
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '=100.0.0',
          version: '100.0.0',
        },
      },
    },
    importers: {
      'project-0': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.0.0',
          },
        },
      },
      'project-1': {},
    },
  }))

  await execPnpm(['add', '--filter=project-1', '--save-catalog', '@pnpm.e2e/foo@100.1.0', '@pnpm.e2e/bar@100.1.0'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '=100.0.0', // unchanged
          version: '100.0.0',
        },
        '@pnpm.e2e/foo': {
          specifier: '100.1.0', // created by `pnpm add --save-catalog`
          version: '100.1.0',
        },
      },
    },
    importers: {
      'project-0': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:', // unchanged
            version: '100.0.0',
          },
        },
      },
      'project-1': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: '100.1.0', // created by `pnpm add --save-catalog`
            version: '100.1.0',
          },
          '@pnpm.e2e/foo': {
            specifier: 'catalog:', // created by `pnpm add --save-catalog`
            version: '100.1.0',
          },
        },
      },
    },
  }))
  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '=100.0.0', // unchanged
      '@pnpm.e2e/foo': '100.1.0', // created by `pnpm add --save-catalog`
    },
    packages: ['project-0', 'project-1'],
  })
  expect(loadJsonFileSync('project-0/package.json')).toStrictEqual(manifests[0])
  expect(loadJsonFileSync('project-1/package.json')).toStrictEqual({
    ...manifests[1],
    dependencies: {
      ...manifests[1].dependencies,
      '@pnpm.e2e/bar': '100.1.0',
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
})

test('--save-catalog creates new workspace manifest with the new catalog (recursive add)', async () => {
  const manifests: ProjectManifest[] = [
    {
      name: 'project-0',
      version: '0.0.0',
    },
    {
      name: 'project-1',
      version: '0.0.0',
    },
  ]

  preparePackages(manifests)

  await execPnpm(['add', '--recursive', '--save-catalog', '@pnpm.e2e/foo@100.1.0'])

  expect(readYamlFileSync('project-0/pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/foo': {
          specifier: '100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
  }))
  expect(readYamlFileSync('project-1/pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/foo': {
          specifier: '100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
  }))

  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/foo': '100.1.0',
    },
  })

  expect(loadJsonFileSync('project-0/package.json')).toStrictEqual({
    ...manifests[0],
    dependencies: {
      ...manifests[0].dependencies,
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
  expect(loadJsonFileSync('project-1/package.json')).toStrictEqual({
    ...manifests[1],
    dependencies: {
      ...manifests[1].dependencies,
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
})

test('--save-catalog-name', async () => {
  const manifest: ProjectManifest = {
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
    dependencies: {
      '@pnpm.e2e/bar': 'catalog:',
    },
  }

  prepare(manifest)

  writeYamlFileSync('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
    },
  }))

  await execPnpm(['add', '--save-catalog-name=my-catalog', '@pnpm.e2e/foo'])
  expect(readYamlFileSync('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/bar': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
      'my-catalog': {
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/bar': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
          '@pnpm.e2e/foo': {
            specifier: 'catalog:my-catalog',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/bar@100.1.0': expect.anything(),
      '@pnpm.e2e/foo@100.1.0': expect.anything(),
    },
  }))
  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
    catalogs: {
      'my-catalog': {
        '@pnpm.e2e/foo': '^100.1.0',
      },
    },
  })
  expect(loadJsonFileSync('package.json')).toStrictEqual({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/foo': 'catalog:my-catalog',
    },
  })
})
