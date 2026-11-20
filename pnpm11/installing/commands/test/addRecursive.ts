import path from 'node:path'

import { expect, test } from '@jest/globals'
import { add, install } from '@pnpm/installing.commands'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectId } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

test('recursive add --save-dev, --save-peer on workspace with multiple lockfiles', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    saveDev: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive@1.0.0'])
  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    savePeer: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative@1.0.0'])

  {
    const { default: manifest } = (await import(path.resolve('project-1/package.json')))
    expect(
      manifest.devDependencies
    ).toEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toEqual(
      { 'is-negative': '1.0.0' }
    )
    expect(
      projects['project-1'].readLockfile().importers['.'].devDependencies
    ).toEqual(
      {
        'is-positive': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
        'is-negative': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
      }
    )
  }

  {
    const { default: manifest } = (await import(path.resolve('project-2/package.json')))
    expect(
      manifest.devDependencies
    ).toEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toEqual(
      { 'is-negative': '1.0.0' }
    )
    expect(
      projects['project-2'].readLockfile().importers['.'].devDependencies
    ).toEqual(
      {
        'is-positive': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
        'is-negative': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
      }
    )
  }
})

test('recursive add --save-dev, --save-peer on workspace with single lockfile', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    saveDev: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive@1.0.0'])
  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    savePeer: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative@1.0.0'])

  {
    const { default: manifest } = (await import(path.resolve('project-1/package.json')))
    expect(
      manifest.devDependencies
    ).toEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toEqual(
      { 'is-negative': '1.0.0' }
    )
  }

  {
    const { default: manifest } = (await import(path.resolve('project-2/package.json')))
    expect(
      manifest.devDependencies
    ).toEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toEqual(
      { 'is-negative': '1.0.0' }
    )
  }

  const lockfile = readYamlFileSync<LockfileObject>('./pnpm-lock.yaml')
  expect(
    lockfile.importers['project-1' as ProjectId].devDependencies
  ).toEqual(
    {
      'is-positive': {
        specifier: '1.0.0',
        version: '1.0.0',
      },
      'is-negative': {
        specifier: '1.0.0',
        version: '1.0.0',
      },
    }
  )
})

// Covers https://github.com/pnpm/pnpm/issues/11225
test.each([
  { recursive: true, dir: '.' },
  { recursive: false, dir: 'project-1' },
])('add keeps the version of a peer dependency the project declares (recursive: $recursive)', async ({ recursive, dir }) => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        '@pnpm/y': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        '@pnpm/y': '2.0.0',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['project-1', 'project-2'],
  })
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    autoInstallPeers: true,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }
  await install.handler({ ...opts, recursive: true, selectedProjectsGraph })

  const { selectedProjectsGraph: filteredGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  await add.handler({
    ...opts,
    dir: path.resolve(dir),
    recursive,
    selectedProjectsGraph: filteredGraph,
  }, ['@pnpm.e2e/has-y-peer@1.0.0'])

  expect(readYamlFileSync<LockfileObject>('pnpm-lock.yaml').importers['project-1' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/has-y-peer': {
      specifier: '1.0.0',
      version: '1.0.0(@pnpm/y@1.0.0)',
    },
    '@pnpm/y': {
      specifier: '1.0.0',
      version: '1.0.0',
    },
  })
})
