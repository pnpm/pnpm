/// <reference path="../../../__typings__/index.d.ts" />
import path from 'path'
import fs from 'fs'
import { licenses } from '@pnpm/plugin-commands-licenses'
import { install } from '@pnpm/plugin-commands-installation'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import stripAnsi from 'strip-ansi'
import { DEFAULT_OPTS } from './utils'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'

const f = fixtures(__dirname)

test('pnpm licenses', async () => {
  const workspaceDir = tempDir()
  f.copy('complex-licenses', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses: show details', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-licenses', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: true,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages-details')
})

test('pnpm licenses: output as json', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-licenses', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    json: true,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(output).not.toHaveLength(0)
  expect(output).not.toBe('No licenses in packages found')
  const parsedOutput = JSON.parse(output)
  expect(parsedOutput).toEqual({
    MIT: [
      {
        name: 'is-positive',
        versions: ['3.1.0'],
        paths: [expect.stringContaining('is-positive@3.1.0')],
        license: 'MIT',
        author: expect.any(String),
        homepage: expect.any(String),
        description: expect.any(String),
      },
    ],
  })
  const packagesWithMIT = parsedOutput['MIT']
  expect(Object.keys(packagesWithMIT[0])).toEqual([
    'name',
    'versions',
    'paths',
    'license',
    'author',
    'homepage',
    'description',
  ])
})

test('pnpm licenses: path should be correct for workspaces', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-licenses', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterPackagesFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const barPackageDir = path.join(workspaceDir, 'bar')
  for (const packageDir of [workspaceDir, barPackageDir]) {
    // eslint-disable-next-line no-await-in-loop
    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir: packageDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      long: false,
      json: true,
      storeDir: path.resolve(storeDir, 'v3'),
    }, ['list'])

    expect(exitCode).toBe(0)

    const parsedOutput = JSON.parse(output)
    for (const license in parsedOutput) {
      const packages = parsedOutput[license]
      for (const pkg of packages) {
        const pkgRoots = pkg['paths']
        expect(pkgRoots).not.toHaveLength(0)
        for (const pkgRoot of pkgRoots) {
          const packageJsonPath = path.join(pkgRoot, 'package.json')
          expect(fs.existsSync(packageJsonPath)).toBeTruthy()
        }
      }
    }
  }
})

test('pnpm licenses: filter outputs', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-licenses', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterPackagesFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const { output, exitCode } = await licenses.handler(
    {
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      pnpmHomeDir: '',
      long: false,
      selectedProjectsGraph: Object.fromEntries(
        Object.entries(selectedProjectsGraph).filter(([path]) =>
          path.includes('bar')
        )
      ),
      storeDir: path.resolve(storeDir, 'v3'),
    }, ['list']
  )

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses: fails when lockfile is missing', async () => {
  await expect(
    licenses.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('./test/fixtures/invalid'),
      pnpmHomeDir: '',
      long: true,
    }, ['list'])
  ).rejects.toThrowErrorMatchingInlineSnapshot(
    '"No pnpm-lock.yaml found: Cannot check a project without a lockfile"'
  )
})

test('pnpm licenses: should correctly read LICENSE file with executable file mode', async () => {
  const workspaceDir = tempDir()
  f.copy('file-mode-test', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  // Attempt to run the licenses command now
  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: true,
    // we need to prefix it with v3 otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages-details')
})

test('pnpm licenses should work with file protocol dependency', async () => {
  const workspaceDir = tempDir()
  f.copy('with-file-protocol', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses should work with git protocol dep that have patches', async () => {
  const workspaceDir = tempDir()
  f.copy('with-git-protocol-patched-deps', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    frozenLockfile: true,
    pnpmHomeDir: '',
    storeDir,
  })

  const { exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
})

test('pnpm licenses should work with git protocol dep that have peerDependencies', async () => {
  const workspaceDir = tempDir()
  f.copy('with-git-protocol-peer-deps', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
})

test('pnpm licenses should work git repository name containing capital letters', async () => {
  const workspaceDir = tempDir()
  f.copy('with-git-protocol-caps', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, 'v3'),
  }, ['list'])

  expect(exitCode).toBe(0)
})
