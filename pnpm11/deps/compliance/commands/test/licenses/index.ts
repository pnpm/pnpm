/// <reference path="../../../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { licenses } from '@pnpm/deps.compliance.commands'
import { install } from '@pnpm/installing.commands'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

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
    // we need to prefix it with STORE_VERSION otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
    // we need to prefix it with STORE_VERSION otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
    // we need to prefix it with STORE_VERSION otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
  const _path = path.join('node_modules', '.pnpm')
  expect(packagesWithMIT[0].paths[0].includes(_path)).toBeTruthy()
})

test('pnpm licenses: paths point at the packages placed by the hoisted linker', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-licenses', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    nodeLinker: 'hoisted',
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    nodeLinker: 'hoisted',
    pnpmHomeDir: '',
    long: false,
    json: true,
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  const parsedOutput = JSON.parse(output)
  expect(parsedOutput.MIT[0].paths).toStrictEqual([path.join(workspaceDir, 'node_modules', 'is-positive')])
})

test('pnpm licenses: path should be correct for workspaces', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-licenses', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

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

  for (const packageDir of [path.join(workspaceDir, 'foo'), path.join(workspaceDir, 'bar')]) {
    // eslint-disable-next-line no-await-in-loop
    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir: packageDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      long: false,
      json: true,
      storeDir: path.resolve(storeDir, STORE_VERSION),
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
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

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
      storeDir: path.resolve(storeDir, STORE_VERSION),
    }, ['list']
  )

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses: lists only the dependencies of the project in the current directory', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-licenses', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

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

  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: path.join(workspaceDir, 'bar'),
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    json: true,
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  const packageNames = Object.values(JSON.parse(output) as Record<string, Array<{ name: string }>>)
    .flat()
    .map(({ name }) => name)
  expect(packageNames).toStrictEqual(['is-positive'])
})

test('pnpm licenses: reads the lockfile of each project in a workspace with dedicated lockfiles', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-licenses', workspaceDir)

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  for (const projectDir of [path.join(workspaceDir, 'foo'), path.join(workspaceDir, 'bar')]) {
    // eslint-disable-next-line no-await-in-loop
    await install.handler({
      ...DEFAULT_OPTS,
      dir: projectDir,
      lockfileDir: projectDir,
      pnpmHomeDir: '',
      storeDir,
    })
  }

  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    json: true,
    recursive: true,
    sharedWorkspaceLockfile: false,
    selectedProjectsGraph,
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  const packages = Object.values(JSON.parse(output) as Record<string, Array<{ name: string, paths: string[] }>>).flat()
  expect(packages.map(({ name }) => name).sort()).toStrictEqual(['is-positive', 'js-tokens', 'loose-envify', 'react', 'react-dom', 'scheduler', 'typescript'])
  for (const { paths } of packages) {
    for (const pkgRoot of paths) {
      expect(fs.existsSync(path.join(pkgRoot, 'package.json'))).toBeTruthy()
    }
  }
})

test('pnpm licenses: keeps packages with the same name and version but different licenses from different lockfiles', async () => {
  const workspaceDir = tempDir()
  const projects = [
    { name: 'foo', dependency: 'local-mit', license: 'MIT' },
    { name: 'bar', dependency: 'local-isc', license: 'ISC' },
  ]
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - foo\n  - bar\n')
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({ private: true }))
  const storeDir = path.join(workspaceDir, 'store')
  for (const { name, dependency, license } of projects) {
    fs.mkdirSync(path.join(workspaceDir, dependency))
    fs.writeFileSync(path.join(workspaceDir, dependency, 'package.json'), JSON.stringify({ name: 'local', version: '1.0.0', license }))
    const projectDir = path.join(workspaceDir, name)
    fs.mkdirSync(projectDir)
    fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({ name, dependencies: { local: `file:../${dependency}` } }))
    // eslint-disable-next-line no-await-in-loop
    await install.handler({
      ...DEFAULT_OPTS,
      dir: projectDir,
      lockfileDir: projectDir,
      pnpmHomeDir: '',
      storeDir,
    })
  }

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])
  const { output, exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    json: true,
    recursive: true,
    sharedWorkspaceLockfile: false,
    selectedProjectsGraph: Object.fromEntries(
      Object.entries(selectedProjectsGraph).filter(([projectDir]) => projectDir !== workspaceDir)
    ),
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  const report = JSON.parse(output) as Record<string, Array<{ name: string }>>
  expect(Object.keys(report).sort()).toStrictEqual(['ISC', 'MIT'])
  expect(report.ISC.map(({ name }) => name)).toStrictEqual(['local'])
  expect(report.MIT.map(({ name }) => name)).toStrictEqual(['local'])
})

test('pnpm licenses: fails when lockfile is missing', async () => {
  const dir = path.resolve('./test/fixtures/invalid')
  await expect(
    licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      long: true,
    }, ['list'])
  ).rejects.toThrow(`No pnpm-lock.yaml found in "${dir}": Cannot check a project without a lockfile`)
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
    // we need to prefix it with STORE_VERSION otherwise licenses tool can't find anything
    // in the content-addressable directory
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})

test('pnpm licenses should work with git protocol dep that have patches', async () => {
  const workspaceDir = tempDir()
  f.copy('with-git-protocol-patched-deps', workspaceDir)
  const patchedDependencies = {
    'is-positive@3.1.0': 'patches/is-positive@3.1.0.patch',
  }

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    frozenLockfile: true,
    patchedDependencies,
    pnpmHomeDir: '',
    storeDir,
  })

  const { exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
    allowBuilds: {
      'ajv-keywords@https://codeload.github.com/ajv-validator/ajv-keywords/tar.gz/a11389b4d1934d360fb2a24dd920ec597295c8fc': true,
      'ajv-keywords@git+https://github.com/ajv-validator/ajv-keywords.git#a11389b4d1934d360fb2a24dd920ec597295c8fc': true,
    },
    pnpmHomeDir: '',
    storeDir,
  })

  const { exitCode } = await licenses.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    long: false,
    storeDir: path.resolve(storeDir, STORE_VERSION),
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
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
})

test('pnpm licenses: reports a runtime downloaded through devEngines', async () => {
  const workspaceDir = tempDir()
  f.copy('with-downloaded-runtime', workspaceDir)

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
    storeDir: path.resolve(storeDir, STORE_VERSION),
  }, ['list'])

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toMatchSnapshot('show-packages')
})
