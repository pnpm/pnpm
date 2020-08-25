import { preparePackages } from '@pnpm/prepare'
import { run } from '@pnpm/plugin-commands-script-runners'
import { filterPkgsBySelectorObjects, readProjects } from '@pnpm/filter-workspace-packages'
import PnpmError from '@pnpm/error'
import { DEFAULT_OPTS, REGISTRY } from './utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('pnpm recursive run', async (t) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output1.json && node -e "process.stdout.write(\'project-1\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output1.json',
        postbuild: 'node -e "process.stdout.write(\'project-2-postbuild\')" | json-append ../output1.json',
        prebuild: 'node -e "process.stdout.write(\'project-2-prebuild\')" | json-append ../output1.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output2.json',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
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

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
  t.end()
})

test('pnpm recursive run concurrently', async (t) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); require(\'json-append\').append(Date.now(),\'../output1.json\');},50)"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); require(\'json-append\').append(Date.now(),\'../output2.json\');},25)"',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
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

  const outputs1 = await import(path.resolve('output1.json')) as number[]
  const outputs2 = await import(path.resolve('output2.json')) as number[]

  t.ok(Math.max(outputs1[0], outputs2[0]) < Math.min(outputs1[outputs1.length - 1], outputs2[outputs2.length - 1]))
  t.end()
})

test('`pnpm recursive run` fails when run without filters and no package has the desired command, unless if-present is set', async (t) => {
  preparePackages(t, [
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  t.comment('recursive run does not fail when if-present is true')
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
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` fails when run with a filter that includes all packages and no package has the desired command, unless if-present is set', async (t) => {
  preparePackages(t, [
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

  t.comment('recursive run does not fail when if-present is true')
  await run.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    ifPresent: true,
    recursive: true,
    workspaceDir: process.cwd(),
  }, ['this-command-does-not-exist'])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    }, ['this-command-does-not-exist'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` succeeds when run against a subset of packages and no package has the desired command', async (t) => {
  preparePackages(t, [
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

  const { allProjects } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
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
  t.end()
})

test('"pnpm run --filter <pkg>" without specifying the script name', async (t) => {
  preparePackages(t, [
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

  const { allProjects } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  t.comment('prints the list of available commands if a single project is selected')
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

    t.equal(output, `\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi`)
  }
  t.comment('throws an error if several projects are selected')
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
    } catch (_err) {
      err = _err
    }

    t.ok(err)
    t.equal(err.code, 'ERR_PNPM_SCRIPT_NAME_IS_REQUIRED')
    t.equal(err.message, 'You must specify the script you want to run')
  }
  t.end()
})

test('testing the bail config with "pnpm recursive run"', async (t) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: 'exit 1 && node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output.json',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
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
    }, ['build', '--no-bail'])
  } catch (_err) {
    err1 = _err
  }
  t.equal(err1.code, 'ERR_PNPM_RECURSIVE_FAIL')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

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
  } catch (_err) {
    err2 = _err
  }

  t.equal(err2.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.end()
})

test('pnpm recursive run with filtering', async (t) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
        postbuild: 'node -e "process.stdout.write(\'project-2-postbuild\')" | json-append ../output.json',
        prebuild: 'node -e "process.stdout.write(\'project-2-prebuild\')" | json-append ../output.json',
      },
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() }
  )
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
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

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
  t.end()
})

test('`pnpm recursive run` should always trust the scripts', async (t) => {
  preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project\')" | json-append ../output.json',
      },
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  process.env['npm_config_unsafe_perm'] = 'false'
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    ...await readProjects(process.cwd(), []),
  }, ['build'])
  delete process.env.npm_config_unsafe_perm

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project'])
  t.end()
})

test('`pnpm run -r` should avoid infinite recursion', async (t) => {
  preparePackages(t, [
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

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output1.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output2.json',
      },
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', {})

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [{ namePattern: 'project-1' }])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: path.resolve('project-1'),
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-2'])
  t.deepEqual(outputs2, ['project-3'])
  t.end()
})
