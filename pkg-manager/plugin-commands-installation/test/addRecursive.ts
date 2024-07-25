import path from 'path'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type Lockfile } from '@pnpm/lockfile-types'
import { add } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectId } from '@pnpm/types'
import { sync as readYamlFile } from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'

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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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
    const manifest = (await import(path.resolve('project-1/package.json')))
    expect(
      manifest.devDependencies
    ).toStrictEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toStrictEqual(
      { 'is-negative': '1.0.0' }
    )
    expect(
      projects['project-1'].readLockfile().importers['.'].devDependencies
    ).toStrictEqual(
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
    const manifest = (await import(path.resolve('project-2/package.json')))
    expect(
      manifest.devDependencies
    ).toStrictEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toStrictEqual(
      { 'is-negative': '1.0.0' }
    )
    expect(
      projects['project-2'].readLockfile().importers['.'].devDependencies
    ).toStrictEqual(
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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
    const manifest = (await import(path.resolve('project-1/package.json')))
    expect(
      manifest.devDependencies
    ).toStrictEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toStrictEqual(
      { 'is-negative': '1.0.0' }
    )
  }

  {
    const manifest = (await import(path.resolve('project-2/package.json')))
    expect(
      manifest.devDependencies
    ).toStrictEqual(
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    expect(
      manifest.peerDependencies
    ).toStrictEqual(
      { 'is-negative': '1.0.0' }
    )
  }

  const lockfile = readYamlFile<Lockfile>('./pnpm-lock.yaml')
  expect(
    lockfile.importers['project-1' as ProjectId].devDependencies
  ).toStrictEqual(
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
