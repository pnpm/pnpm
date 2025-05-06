import { type LockfileFile } from '@pnpm/lockfile.types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectManifest } from '@pnpm/types'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

const SAVE_CATALOG = ['--save-catalog'] as const

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

  writeYamlFile('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFile('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
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
  } as Partial<LockfileFile>))

  await execPnpm(['add', ...SAVE_CATALOG, '@pnpm.e2e/foo'])
  expect(readYamlFile('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
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
  } as Partial<LockfileFile>))
  expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
      '@pnpm.e2e/foo': '^100.1.0',
    },
  })
  expect(loadJsonFile('package.json')).toStrictEqual({
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

  writeYamlFile('pnpm-workspace.yaml', {
    sharedWorkspaceLockfile: true,
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
    packages: ['project-0', 'project-1'],
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['install'])
  expect(readYamlFile('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
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
  } as Partial<LockfileFile>))

  await execPnpm(['--filter=project-1', 'add', ...SAVE_CATALOG, '@pnpm.e2e/foo'])
  expect(readYamlFile('pnpm-lock.yaml')).toStrictEqual(expect.objectContaining({
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
  } as Partial<LockfileFile>))
  expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/bar': '^100.1.0',
      '@pnpm.e2e/foo': '^100.1.0',
    },
    packages: ['project-0', 'project-1'],
    sharedWorkspaceLockfile: true,
  })
  expect(loadJsonFile('project-1/package.json')).toStrictEqual({
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

  writeYamlFile('pnpm-workspace.yaml', {
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

    const lockfile0: LockfileFile = readYamlFile('project-0/pnpm-lock.yaml')
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

    const lockfile1: LockfileFile = readYamlFile('project-1/pnpm-lock.yaml')
    expect(lockfile1.catalogs).toBeUndefined()
    expect(lockfile1.importers).toStrictEqual({
      '.': {},
    } as LockfileFile['importers'])
  }

  {
    await execPnpm(['--filter=project-1', 'add', ...SAVE_CATALOG, '@pnpm.e2e/foo'])

    const lockfile0: LockfileFile = readYamlFile('project-0/pnpm-lock.yaml')
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

    const lockfile1: LockfileFile = readYamlFile('project-1/pnpm-lock.yaml')
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

    expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
      catalog: {
        '@pnpm.e2e/bar': '^100.1.0',
        '@pnpm.e2e/foo': '^100.1.0',
      },
      packages: ['project-0', 'project-1'],
      sharedWorkspaceLockfile: false,
    })

    expect(loadJsonFile('project-1/package.json')).toStrictEqual({
      ...manifests[1],
      dependencies: {
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

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['project-0', 'project-1'],
  })

  await execPnpm(['install'])
  expect(readYamlFile('pnpm-lock.yaml')).toMatchObject({
    importers: {
      'project-0': {},
      'project-1': {},
    },
  } as Partial<LockfileFile>)

  await execPnpm(['--filter=project-1', 'add', ...SAVE_CATALOG, 'project-0@workspace:*'])
  expect(readYamlFile('pnpm-lock.yaml')).toMatchObject({
    importers: {
      'project-0': {},
      'project-1': {
        dependencies: {
          'project-0': {
            specifier: 'workspace:*',
            version: 'link:../project-0',
          },
        },
      },
    },
  } as Partial<LockfileFile>)
  expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
    packages: ['project-0', 'project-1'],
  })
  expect(loadJsonFile('project-1/package.json')).toStrictEqual({
    ...manifests[1],
    dependencies: {
      'project-0': 'workspace:*',
    },
  })
})
