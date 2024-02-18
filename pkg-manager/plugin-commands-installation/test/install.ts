import fs from 'fs'
import delay from 'delay'
import path from 'path'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { sync as rimraf } from '@zkochan/rimraf'
import { DEFAULT_OPTS } from './utils'

test('install fails if no package.json is found', async () => {
  prepareEmpty()

  await expect(install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })).rejects.toThrow(/No package\.json found/)
})

test('install does not fail when a new package is added', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  const pkg = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('install with no store integrity validation', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  // We should have a short delay before modifying the file in the store.
  // Otherwise pnpm will not consider it to be modified.
  await delay(200)
  const readmePath = path.join(DEFAULT_OPTS.storeDir, 'v3/files/9a/f6af85f55c111108eddf1d7ef7ef224b812e7c7bfabae41c79cf8bc9a910352536963809463e0af2799abacb975f22418a35a1d170055ef3fdc3b2a46ef1c5')
  fs.writeFileSync(readmePath, 'modified', 'utf8')

  rimraf('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    verifyStoreIntegrity: false,
  })

  expect(fs.readFileSync('node_modules/is-positive/readme.md', 'utf8')).toBe('modified')
})

describe('install injected dependency with publishConfig in manifest', () => {
  test.each([true, false])('with shared lockfile: %s', async withLockfile => {
    preparePackages([
      {
        location: '.',
        package: {
          name: 'root',
          version: '1.0.0',
        },
      },
      {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          'project-2': 'workspace:*',
        },
        devDependencies: {
          'project-3': 'workspace:*',
        },
        dependenciesMeta: {
          'project-2': {
            injected: true,
          },
        },
      },
      {
        name: 'project-2',
        version: '1.0.0',
        devDependencies: {
          'project-3': 'workspace:*',
        },
        publishConfig: {
          exports: {
            foo: 'bar',
          },
        },
      },
      {
        location: 'nested-packages/project-3',
        package: {
          name: 'project-3',
          version: '1.0.0',
        },
      },
    ])

    const { allProjects, allProjectsGraph, selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const lockfileDir = withLockfile ? process.cwd() : undefined

    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    })

    const originalProject2 = fs.readFileSync('project-2/package.json', 'utf8')
    expect(
      JSON.parse(originalProject2)
    ).toEqual({
      name: 'project-2',
      version: '1.0.0',
      devDependencies: {
        'project-3': 'workspace:*',
      },
      publishConfig: {
        exports: {
          foo: 'bar',
        },
      },
    })

    const injectedProject2 = fs.readFileSync('project-1/node_modules/project-2/package.json', 'utf8')
    expect(
      JSON.parse(injectedProject2)
    ).toEqual({
      name: 'project-2',
      version: '1.0.0',
      devDependencies: {
        'project-3': 'workspace:*',
      },
      publishConfig: {
        exports: {
          foo: 'bar',
        },
      },
    })

    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      allProjectsGraph,
      dir: process.cwd(),
      lockfileDir,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    })
  })
})
