import { promises as fs } from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { run } from '@pnpm/plugin-commands-script-runners'
import { filterPkgsBySelectorObjects, readProjects } from '@pnpm/filter-workspace-packages'
import { type PnpmError } from '@pnpm/error'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS, REGISTRY_URL } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('pnpm recursive run', async () => {
  preparePackages([
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

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1', 'project-2'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive run with enable-pre-post-scripts', async () => {
  preparePackages([
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

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive run reversed', async () => {
  preparePackages([
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

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-2', 'project-1'])
  expect(outputs2).toStrictEqual(['project-3', 'project-1'])
})

test('pnpm recursive run concurrently', async () => {
  preparePackages([
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

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
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

  const { allProjects } = await readProjects(process.cwd(), [])
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

  const { allProjects } = await readProjects(process.cwd(), [])
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
  preparePackages([
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
    }, ['build', '--no-bail'])
  } catch (_err: any) { // eslint-disable-line
    err1 = _err
  }
  expect(err1.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual(['project-1', 'project-3'])

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
  } catch (_err: any) { // eslint-disable-line
    err2 = _err
  }

  expect(err2.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
})

test('pnpm recursive run with filtering', async () => {
  preparePackages([
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

  const { default: outputs } = await import(path.resolve('output.json'))

  expect(outputs).toStrictEqual(['project-1'])
})

test('`pnpm recursive run` should always trust the scripts', async () => {
  preparePackages([
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
    ...await readProjects(process.cwd(), []),
  }, ['build'])
  delete process.env.npm_config_unsafe_perm

  const { default: outputs } = await import(path.resolve('output.json'))

  expect(outputs).toStrictEqual(['project'])
})

test('`pnpm run -r` should avoid infinite recursion', async () => {
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
    REGISTRY_URL,
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

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-2'])
  expect(outputs2).toStrictEqual(['project-3'])
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
      ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
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
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output1.json',
      },
      dependencies: {
        'json-append': '1',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output1.json',
      },
      dependencies: {
        'project-1': '1',
        'json-append': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output1.json',
      },
      dependencies: {
        'project-1': '1',
        'json-append': '1',
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
    ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    recursive: true,
    resumeFrom: 'project-3',
    workspaceDir: process.cwd(),
  }, ['build'])

  const { default: output1 } = await import(path.resolve('output1.json'))
  expect(output1).not.toContain('project-1')
  expect(output1).toContain('project-2')
  expect(output1).toContain('project-3')
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
    ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
    dir: process.cwd(),
    recursive: true,
    rootProjectManifest: {
      name: 'test-workspaces',
      private: true,
    },
    workspaceDir: process.cwd(),
  }, ['/^(lint|build):.*/'])
  expect(await fs.readFile('output-build-1-a.txt', { encoding: 'utf-8' })).toEqual('1-a')
  expect(await fs.readFile('output-build-1-b.txt', { encoding: 'utf-8' })).toEqual('1-b')
  expect(await fs.readFile('output-build-1-c.txt', { encoding: 'utf-8' })).toEqual('1-c')
  expect(await fs.readFile('output-build-2-a.txt', { encoding: 'utf-8' })).toEqual('2-a')
  expect(await fs.readFile('output-build-2-b.txt', { encoding: 'utf-8' })).toEqual('2-b')
  expect(await fs.readFile('output-build-2-c.txt', { encoding: 'utf-8' })).toEqual('2-c')
  expect(await fs.readFile('output-build-3-a.txt', { encoding: 'utf-8' })).toEqual('3-a')
  expect(await fs.readFile('output-build-3-b.txt', { encoding: 'utf-8' })).toEqual('3-b')
  expect(await fs.readFile('output-build-3-c.txt', { encoding: 'utf-8' })).toEqual('3-c')

  expect(await fs.readFile('output-lint-1-a.txt', { encoding: 'utf-8' })).toEqual('1-a')
  expect(await fs.readFile('output-lint-1-b.txt', { encoding: 'utf-8' })).toEqual('1-b')
  expect(await fs.readFile('output-lint-1-c.txt', { encoding: 'utf-8' })).toEqual('1-c')
  expect(await fs.readFile('output-lint-2-a.txt', { encoding: 'utf-8' })).toEqual('2-a')
  expect(await fs.readFile('output-lint-2-b.txt', { encoding: 'utf-8' })).toEqual('2-b')
  expect(await fs.readFile('output-lint-2-c.txt', { encoding: 'utf-8' })).toEqual('2-c')
  expect(await fs.readFile('output-lint-3-a.txt', { encoding: 'utf-8' })).toEqual('3-a')
  expect(await fs.readFile('output-lint-3-b.txt', { encoding: 'utf-8' })).toEqual('3-b')
  expect(await fs.readFile('output-lint-3-c.txt', { encoding: 'utf-8' })).toEqual('3-c')
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
      ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
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
      ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
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
