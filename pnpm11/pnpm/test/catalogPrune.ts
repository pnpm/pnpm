import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { rimrafSync } from '@zkochan/rimraf'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

const CATALOG = {
  '@pnpm.e2e/foo': '100.0.0',
  '@pnpm.e2e/bar': '100.0.0',
}

const CATALOGS = {
  extra: {
    '@pnpm.e2e/qar': '100.0.0',
  },
}

test.each([
  [[]],
  [['--lockfile-only']],
])('a filtered frozen install keeps the catalog entries the lockfile records of a project that is absent from disk (args: %j)', async (args: string[]) => {
  await preparePartialWorkspace()

  await execPnpm(['install', '--frozen-lockfile', '--filter', 'project-1', ...args])
  await execPnpm(['install', '--frozen-lockfile', ...args])

  expectWorkspaceCatalogs({ catalog: CATALOG, catalogs: CATALOGS })
})

test.each([
  [[], {}],
  [['--lockfile-only'], {}],
  [[], { CI: 'true' }],
])('a filtered install that finds the lockfile up to date keeps the catalog entries the lockfile records of a project that is absent from disk (args: %j, env: %j)', async (args: string[], env: Record<string, string>) => {
  await preparePartialWorkspace()

  await execPnpm(['install', '--filter', 'project-1', ...args], { env })
  await execPnpm(['install', '--frozen-lockfile'])

  expectWorkspaceCatalogs({ catalog: CATALOG, catalogs: CATALOGS })
})

test('a filtered remove keeps the catalog entries the lockfile records of a project that is absent from disk', async () => {
  await preparePartialWorkspace()

  await execPnpm(['remove', '@pnpm.e2e/dep-of-pkg-with-1-dep', '--filter', 'project-1'])
  await execPnpm(['install', '--frozen-lockfile'])

  expectWorkspaceCatalogs({ catalog: CATALOG, catalogs: CATALOGS })
})

test.each([
  [['install', '--frozen-lockfile']],
  [['remove', '@pnpm.e2e/dep-of-pkg-with-1-dep']],
])('a command run inside a project keeps the catalog entries the lockfile records of a project that is absent from disk (args: %j)', async (args: string[]) => {
  await preparePartialWorkspace()

  process.chdir('project-1')
  await execPnpm(args)
  process.chdir('..')
  await execPnpm(['install', '--frozen-lockfile'])

  expectWorkspaceCatalogs({ catalog: CATALOG, catalogs: CATALOGS })
})

test('a filtered install with the lockfile disabled prunes the catalog entries of a project that is absent from disk', async () => {
  await prepareWorkspace()
  await execPnpm(['install', '--frozen-lockfile'])
  writeWorkspaceManifest({ catalog: CATALOG, catalogs: CATALOGS }, { lockfile: false })
  rimrafSync('project-2')

  await execPnpm(['install', '--filter', 'project-1'])

  expectWorkspaceCatalogs({ catalog: { '@pnpm.e2e/foo': '100.0.0' }, catalogs: undefined })
})

test('a frozen install prunes a catalog entry that no project references and the lockfile does not record', async () => {
  await prepareWorkspace()
  writeWorkspaceManifest({
    catalog: {
      ...CATALOG,
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
    catalogs: CATALOGS,
  })

  await execPnpm(['install', '--frozen-lockfile'])

  expectWorkspaceCatalogs({ catalog: CATALOG, catalogs: CATALOGS })
})

async function preparePartialWorkspace (): Promise<void> {
  await prepareWorkspace()
  rimrafSync('project-2')
}

async function prepareWorkspace (): Promise<void> {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/bar': 'catalog:',
        '@pnpm.e2e/qar': 'catalog:extra',
      },
    },
  ])
  writeWorkspaceManifest({ catalog: CATALOG, catalogs: CATALOGS })
  await execPnpm(['install', '--lockfile-only'])
}

function writeWorkspaceManifest (
  catalogs: Pick<WorkspaceManifest, 'catalog' | 'catalogs'>,
  settings?: { lockfile: boolean }
): void {
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    catalogPrune: true,
    ...settings,
    ...catalogs,
  })
}

function expectWorkspaceCatalogs (catalogs: Pick<WorkspaceManifest, 'catalog' | 'catalogs'>): void {
  const workspaceManifest = readYamlFileSync<WorkspaceManifest>('pnpm-workspace.yaml')
  expect({ catalog: workspaceManifest.catalog, catalogs: workspaceManifest.catalogs }).toStrictEqual(catalogs)
}
