import { promises as fs } from 'fs'
import path from 'path'
import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { exec, run } from '@pnpm/plugin-commands-script-runners'
import prepare, { prepareEmpty, preparePackages } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import { DEFAULT_OPTS, REGISTRY } from './utils'

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.cjs')

test('pnpm recursive exec', async () => {
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
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})

test('exec inside a workspace package', async () => {
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
  ])

  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project-1'),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['npm', 'run', 'build'])

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1'])
  expect(outputs2).toStrictEqual(['project-1'])
})

test('pnpm recursive exec sets PNPM_PACKAGE_NAME env var', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['node', '-e', 'require(\'fs\').writeFileSync(\'pkgname\', process.env.PNPM_PACKAGE_NAME, \'utf8\')'])

  expect(await fs.readFile('foo/pkgname', 'utf8')).toBe('foo')
})

test('testing the bail config with "pnpm recursive exec"', async () => {
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

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let failed = false
  let err1!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build', '--no-bail'])
  } catch (_err: any) { // eslint-disable-line
    err1 = _err
    failed = true
  }
  expect(err1.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(failed).toBeTruthy()

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual(['project-1', 'project-3'])

  await rimraf('./output.json')

  failed = false
  let err2!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build'])
  } catch (_err: any) { // eslint-disable-line
    err2 = _err
    failed = true
  }

  expect(err2.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(failed).toBeTruthy()
})

test('pnpm recursive exec --no-sort', async () => {
  preparePackages([
    {
      name: 'a-dependent',
      version: '1.0.0',

      dependencies: {
        'b-dependency': '1.0.0',
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'a-dependent\')" | json-append ../output.json',
      },
    },
    {
      name: 'b-dependency',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'b-dependency\')" | json-append ../output.json',
      },
    },
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sort: false,
    workspaceConcurrency: 1,
  }, ['npm', 'run', 'build'])

  const { default: outputs } = await import(path.resolve('output.json'))

  expect(outputs).toStrictEqual(['a-dependent', 'b-dependency'])
})

test('pnpm recursive exec --reverse', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output1.json',
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
        build: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output1.json',
      },
    },
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph,
    recursive: true,
    sort: true,
    reverse: true,
  }, ['npm', 'run', 'build'])

  const { default: outputs1 } = await import(path.resolve('output1.json'))

  expect(outputs1[outputs1.length - 1]).toBe('project-1')
})

test('pnpm exec on single project', async () => {
  prepare({})

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['node', '-e', 'require("fs").writeFileSync("output.json", "[]", "utf8")'])

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual([])
})

test('pnpm exec on single project should return non-zero exit code when the process fails', async () => {
  prepare({})

  {
    const { exitCode } = await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: false,
      selectedProjectsGraph: {},
    }, ['node', '-e', 'process.exitCode=1'])

    expect(exitCode).toBe(1)
  }

  {
    const runResult = await run.handler({
      ...DEFAULT_OPTS,
      argv: {
        original: ['pnpm', 'node', '-e', 'process.exitCode=1'],
      },
      dir: process.cwd(),
      fallbackCommandUsed: true,
      recursive: false,
      selectedProjectsGraph: {},
    }, ['node'])

    expect(runResult['exitCode']).toBe(1)
  }
})

test('pnpm exec outside of projects', async () => {
  prepareEmpty()

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['node', '-e', 'require("fs").writeFileSync("output.json", "[]", "utf8")'])

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual([])
})

test('pnpm recursive exec works with PnP', async () => {
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
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ], {
    env: {
      NPM_CONFIG_NODE_LINKER: 'pnp',
      NPM_CONFIG_SYMLINK: 'false',
    },
  })
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})
