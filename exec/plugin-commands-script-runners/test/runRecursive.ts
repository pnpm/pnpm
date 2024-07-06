import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { run } from '@pnpm/plugin-commands-script-runners'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type PnpmError } from '@pnpm/error'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import execa from 'execa'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS, REGISTRY_URL } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('pnpm recursive run', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive run with enable-pre-post-scripts', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    enablePrePostScripts: true,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive run reversed', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    reverse: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server1.getLines()).toStrictEqual(['project-2', 'project-1'])
  expect(server2.getLines()).toStrictEqual(['project-3', 'project-1'])
})

test('pnpm recursive run concurrently', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); console.log(Date.now());},50)" | ${server1.generateSendStdinScript()}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      scripts: {
        build: `node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); console.log(Date.now());},25)" | ${server2.generateSendStdinScript()}`,
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  const outputs1 = server1.getLines().map(x => Number.parseInt(x))
  const outputs2 = server2.getLines().map(x => Number.parseInt(x))

  expect(Math.max(outputs1[0], outputs2[0]) < Math.min(outputs1[outputs1.length - 1], outputs2[outputs2.length - 1])).toBeTruthy()
})

test('`pnpm recursive run` fails when run without filters and no package has the desired command, unless if-present is set', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  console.log('recursive run does not fail when if-present is true')
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    ifPresent: true,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['this-command-does-not-exist'])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['this-command-does-not-exist'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
})

test('`pnpm recursive run` fails when run with a filter that includes all packages and no package has the desired command, unless if-present is set', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  console.log('recursive run does not fail when if-present is true')
  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    ifPresent: true,
    recursive: true,
    workspaceDir: process.cwd(),
  }, ['this-command-does-not-exist'])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    }, ['this-command-does-not-exist'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
})

test('`pnpm recursive run` succeeds when run against a subset of packages and no package has the desired command', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  const { allProjects } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() }
  )
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['this-command-does-not-exist'])
})

test('"pnpm run --filter <pkg>" without specifying the script name', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        foo: 'echo hi',
        test: 'ts-node test',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  const { allProjects } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  console.log('prints the list of available commands if a single project is selected')
  {
    const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
      allProjects,
      [{ namePattern: 'project-1' }],
      { workspaceDir: process.cwd() }
    )
    const output = await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, [])

    expect(output).toBe(`\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi`)
  }
  console.log('throws an error if several projects are selected')
  {
    const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
      allProjects,
      [{ includeDependents: true, namePattern: 'project-1' }],
      { workspaceDir: process.cwd() }
    )

    let err!: PnpmError
    try {
      await run.handler({
        ...DEFAULT_OPTS,
        allProjects,
        dir: process.cwd(),
        recursive: true,
        selectedProjectsGraph,
        workspaceDir: process.cwd(),
      }, [])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }

    expect(err).toBeTruthy()
    expect(err.code).toBe('ERR_PNPM_SCRIPT_NAME_IS_REQUIRED')
    expect(err.message).toBe('You must specify the script you want to run')
  }
})

test('testing the bail config with "pnpm recursive run"', async () => {
  await using server = await createTestIpcServer()
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: `exit 1 && ${server.sendLineScript('project-2')}`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-3'),
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let err1!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      bail: false,
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err1 = _err
  }
  expect(err1.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  expect(server.getLines()).toStrictEqual(['project-1', 'project-3'])

  let err2!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err2 = _err
  }

  expect(err2.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
})

test('pnpm recursive run with filtering', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-2'),
        postbuild: server.sendLineScript('project-2-postbuild'),
        prebuild: server.sendLineScript('project-2-prebuild'),
      },
    },
  ])

  const { allProjects } = await filterPackagesFromDir(process.cwd(), [])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() }
  )
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server.getLines()).toStrictEqual(['project-1'])
})

test('`pnpm recursive run` should always trust the scripts', async () => {
  await using server = await createTestIpcServer()
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('project'),
      },
    },
  ])

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  process.env['npm_config_unsafe_perm'] = 'false'
  await run.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    ...await filterPackagesFromDir(process.cwd(), []),
  }, ['build'])
  delete process.env.npm_config_unsafe_perm

  expect(server.getLines()).toStrictEqual(['project'])
})

test('`pnpm run -r` should avoid infinite recursion', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `node ${pnpmBin} run -r build`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      scripts: {
        build: server1.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
  ])
  writeYamlFile('pnpm-workspace.yaml', {})

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: path.resolve('project-1'),
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server1.getLines()).toStrictEqual(['project-2'])
  expect(server2.getLines()).toStrictEqual(['project-3'])
})

test('`pnpm recursive run` should fail when no script in package with requiredScripts', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'echo 2',
      },
      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'project-1': '1',
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      rootProjectManifest: {
        name: 'test-workspaces',
        private: true,
        pnpm: {
          requiredScripts: ['build'],
        },
      },
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.message).toContain('Missing script "build" in packages: project-1, project-3')
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
})

test('`pnpm -r --resume-from run` should executed from given package', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-2'),
      },
      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-3'),
      },
      dependencies: {
        'project-1': '1',
      },
    },
  ])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    recursive: true,
    resumeFrom: 'project-3',
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server.getLines().sort()).toEqual(['project-2', 'project-3'])
})

test('pnpm run with RegExp script selector should work on recursive', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        'build:a': 'node -e "require(\'fs\').writeFileSync(\'../output-build-1-a.txt\', \'1-a\', \'utf8\')"',
        'build:b': 'node -e "require(\'fs\').writeFileSync(\'../output-build-1-b.txt\', \'1-b\', \'utf8\')"',
        'build:c': 'node -e "require(\'fs\').writeFileSync(\'../output-build-1-c.txt\', \'1-c\', \'utf8\')"',
        build: 'node -e "require(\'fs\').writeFileSync(\'../output-build-1-a.txt\', \'should not run\', \'utf8\')"',
        'lint:a': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-1-a.txt\', \'1-a\', \'utf8\')"',
        'lint:b': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-1-b.txt\', \'1-b\', \'utf8\')"',
        'lint:c': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-1-c.txt\', \'1-c\', \'utf8\')"',
        lint: 'node -e "require(\'fs\').writeFileSync(\'../output-lint-1-a.txt\', \'should not run\', \'utf8\')"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        'build:a': 'node -e "require(\'fs\').writeFileSync(\'../output-build-2-a.txt\', \'2-a\', \'utf8\')"',
        'build:b': 'node -e "require(\'fs\').writeFileSync(\'../output-build-2-b.txt\', \'2-b\', \'utf8\')"',
        'build:c': 'node -e "require(\'fs\').writeFileSync(\'../output-build-2-c.txt\', \'2-c\', \'utf8\')"',
        build: 'node -e "require(\'fs\').writeFileSync(\'../output-build-2-a.txt\', \'should not run\', \'utf8\')"',
        'lint:a': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-2-a.txt\', \'2-a\', \'utf8\')"',
        'lint:b': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-2-b.txt\', \'2-b\', \'utf8\')"',
        'lint:c': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-2-c.txt\', \'2-c\', \'utf8\')"',
        lint: 'node -e "require(\'fs\').writeFileSync(\'../output-lint-2-a.txt\', \'should not run\', \'utf8\')"',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        'build:a': 'node -e "require(\'fs\').writeFileSync(\'../output-build-3-a.txt\', \'3-a\', \'utf8\')"',
        'build:b': 'node -e "require(\'fs\').writeFileSync(\'../output-build-3-b.txt\', \'3-b\', \'utf8\')"',
        'build:c': 'node -e "require(\'fs\').writeFileSync(\'../output-build-3-c.txt\', \'3-c\', \'utf8\')"',
        build: 'node -e "require(\'fs\').writeFileSync(\'../output-build-3-a.txt\', \'should not run\', \'utf8\')"',
        'lint:a': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-3-a.txt\', \'3-a\', \'utf8\')"',
        'lint:b': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-3-b.txt\', \'3-b\', \'utf8\')"',
        'lint:c': 'node -e "require(\'fs\').writeFileSync(\'../output-lint-3-c.txt\', \'3-c\', \'utf8\')"',
        lint: 'node -e "require(\'fs\').writeFileSync(\'../output-lint-3-a.txt\', \'should not run\', \'utf8\')"',
      },
    },
  ])

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    recursive: true,
    rootProjectManifest: {
      name: 'test-workspaces',
      private: true,
    },
    workspaceDir: process.cwd(),
  }, ['/^(lint|build):.*/'])
  expect(fs.readFileSync('output-build-1-a.txt', { encoding: 'utf-8' })).toEqual('1-a')
  expect(fs.readFileSync('output-build-1-b.txt', { encoding: 'utf-8' })).toEqual('1-b')
  expect(fs.readFileSync('output-build-1-c.txt', { encoding: 'utf-8' })).toEqual('1-c')
  expect(fs.readFileSync('output-build-2-a.txt', { encoding: 'utf-8' })).toEqual('2-a')
  expect(fs.readFileSync('output-build-2-b.txt', { encoding: 'utf-8' })).toEqual('2-b')
  expect(fs.readFileSync('output-build-2-c.txt', { encoding: 'utf-8' })).toEqual('2-c')
  expect(fs.readFileSync('output-build-3-a.txt', { encoding: 'utf-8' })).toEqual('3-a')
  expect(fs.readFileSync('output-build-3-b.txt', { encoding: 'utf-8' })).toEqual('3-b')
  expect(fs.readFileSync('output-build-3-c.txt', { encoding: 'utf-8' })).toEqual('3-c')

  expect(fs.readFileSync('output-lint-1-a.txt', { encoding: 'utf-8' })).toEqual('1-a')
  expect(fs.readFileSync('output-lint-1-b.txt', { encoding: 'utf-8' })).toEqual('1-b')
  expect(fs.readFileSync('output-lint-1-c.txt', { encoding: 'utf-8' })).toEqual('1-c')
  expect(fs.readFileSync('output-lint-2-a.txt', { encoding: 'utf-8' })).toEqual('2-a')
  expect(fs.readFileSync('output-lint-2-b.txt', { encoding: 'utf-8' })).toEqual('2-b')
  expect(fs.readFileSync('output-lint-2-c.txt', { encoding: 'utf-8' })).toEqual('2-c')
  expect(fs.readFileSync('output-lint-3-a.txt', { encoding: 'utf-8' })).toEqual('3-a')
  expect(fs.readFileSync('output-lint-3-b.txt', { encoding: 'utf-8' })).toEqual('3-b')
  expect(fs.readFileSync('output-lint-3-c.txt', { encoding: 'utf-8' })).toEqual('3-c')
})

test('pnpm recursive run report summary', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-1\'), 1000)"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-3\'), 1000)"',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-5',
      version: '1.0.0',
    },
  ])
  let error
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      reportSummary: true,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  const { default: { executionStatus } } = (await import(path.resolve('pnpm-exec-summary.json')))
  expect(executionStatus[path.resolve('project-1')].status).toBe('passed')
  expect(executionStatus[path.resolve('project-1')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-2')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-2')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-3')].status).toBe('passed')
  expect(executionStatus[path.resolve('project-3')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-4')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-4')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-5')].status).toBe('skipped')
})

test('pnpm recursive run report summary with --bail', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-1\'), 1000)"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-3\'), 1000)"',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-5',
      version: '1.0.0',
    },
  ])
  let error
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterPackagesFromDir(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      reportSummary: true,
      workspaceDir: process.cwd(),
      bail: true,
      workspaceConcurrency: 3,
    }, ['build'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error.code).toBe('ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL')

  const { default: { executionStatus } } = (await import(path.resolve('pnpm-exec-summary.json')))

  expect(executionStatus[path.resolve('project-1')].status).toBe('running')
  expect(executionStatus[path.resolve('project-2')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-2')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-3')].status).toBe('running')
  expect(executionStatus[path.resolve('project-4')].status).toBe('queued')
  expect(executionStatus[path.resolve('project-5')].status).toBe('skipped')
})
