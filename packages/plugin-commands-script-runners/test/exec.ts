import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { exec } from '@pnpm/plugin-commands-script-runners'
import { preparePackages } from '@pnpm/prepare'
import { DEFAULT_OPTS, REGISTRY } from './utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import fs = require('mz/fs')
import test = require('tape')

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('pnpm recursive exec', async (t) => {
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
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])

  t.end()
})

test('pnpm recursive exec sets PNPM_PACKAGE_NAME env var', async (t) => {
  preparePackages(t, [
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])

  const { selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await exec.handler({
    ...DEFAULT_OPTS,
    recursive: true,
    selectedProjectsGraph,
  }, ['node', '-e', 'require(\'fs\').writeFileSync(\'pkgname\', process.env.PNPM_PACKAGE_NAME, \'utf8\')'])

  t.equal(await fs.readFile('foo/pkgname', 'utf8'), 'foo', '$PNPM_PACKAGE_NAME is correct')
  t.end()
})

test('testing the bail config with "pnpm recursive exec"', async (t) => {
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
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build', '--no-bail'])
  } catch (_err) {
    err1 = _err
    failed = true
  }
  t.equal(err1.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.ok(failed, 'recursive exec failed with --no-bail')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  failed = false
  let err2!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build'])
  } catch (_err) {
    err2 = _err
    failed = true
  }

  t.equal(err2.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.ok(failed, 'recursive exec failed with --bail')
  t.end()
})

test('pnpm recursive exec --no-sort', async (t) => {
  preparePackages(t, [
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
    recursive: true,
    selectedProjectsGraph,
    sort: false,
    workspaceConcurrency: 1,
  }, ['npm', 'run', 'build'])

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['a-dependent', 'b-dependency'])
  t.end()
})

test('pnpm exec fails without the recursive=true option', async (t) => {
  preparePackages(t, [])

  let err!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      recursive: false,
      selectedProjectsGraph: {},
    }, ['npm', 'run', 'build'])
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_EXEC_NOT_RECURSIVE')

  t.end()
})

test('pnpm recursive exec works with PnP', async (t) => {
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
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])

  t.end()
})
