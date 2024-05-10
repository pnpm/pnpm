import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from '../utils'

test('overrides with local file and link specs', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
      },
    },
    {
      location: 'packages/main',
      package: {
        name: 'main',
        dependencies: {
          'relative-file-pkg': '*',
          'absolute-file-pkg': '*',
          'relative-link-pkg': '*',
          'absolute-link-pkg': '*',
        },
      },
    },
    {
      location: 'packages/indirect',
      package: {
        name: 'indirect',
        dependencies: {
          '@pnpm.e2e/depends-on-pkg-abcd': '1.0.0',
        },
      },
    },
    {
      location: 'overrides/pkg',
      package: {
        name: 'pkg',
        version: '0.0.0',
      },
    },
  ])

  // overrides must be written after prepare because it depends on cwd
  projects.root.writePackageJson({
    name: 'root',
    pnpm: {
      overrides: {
        'relative-file-pkg': 'file:./overrides/pkg',
        'absolute-file-pkg': `file:${path.resolve('overrides/pkg')}`,
        'relative-link-pkg': 'link:./overrides/pkg',
        'absolute-link-pkg': `link:${path.resolve('overrides/pkg')}`,
        '@pnpm.e2e/pkg-a': 'file:./overrides/pkg',
        '@pnpm.e2e/pkg-b': `file:${path.resolve('overrides/pkg')}`,
        '@pnpm.e2e/pkg-c': 'link:./overrides/pkg',
        '@pnpm.e2e/pkg-d': `link:${path.resolve('overrides/pkg')}`,
      },
    },
  })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['packages/*'] })

  await execPnpm(['install'])

  const lockfile = projects.root.readLockfile()

  expect(lockfile.importers['packages/main']).toStrictEqual({
    dependencies: {
      'relative-file-pkg': {
        specifier: 'file:../../overrides/pkg',
        version: 'pkg@file:overrides/pkg',
      },
      'absolute-file-pkg': {
        specifier: `file:${path.resolve('overrides/pkg')}`,
        version: 'pkg@file:overrides/pkg',
      },
      'relative-link-pkg': {
        specifier: 'link:../../overrides/pkg',
        version: 'link:../../overrides/pkg',
      },
      'absolute-link-pkg': {
        specifier: `link:${path.resolve('overrides/pkg')}`,
        version: 'link:../../overrides/pkg',
      },
    },
  } as typeof lockfile['importers'][string])

  expect(lockfile.snapshots['@pnpm.e2e/depends-on-pkg-abcd@1.0.0']).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/pkg-a': 'pkg@file:overrides/pkg',
      '@pnpm.e2e/pkg-b': 'pkg@file:overrides/pkg',
      '@pnpm.e2e/pkg-c': 'link:overrides/pkg',
      '@pnpm.e2e/pkg-d': 'link:overrides/pkg',
    },
  } as typeof lockfile['snapshots'][string])
})
