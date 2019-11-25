import PnpmError from '@pnpm/error'
import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

test('pnpm recursive run', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output1.json`,
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output1.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await recursive.handler(['run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
  t.end()
})

test('pnpm recursive run concurrently', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); require('json-append').append(Date.now(),'../output1.json');},50)"`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); require('json-append').append(Date.now(),'../output2.json');},25)"`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await recursive.handler(['run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const outputs1 = await import(path.resolve('output1.json')) as number[]
  const outputs2 = await import(path.resolve('output2.json')) as number[]

  t.ok(Math.max(outputs1[0], outputs2[0]) < Math.min(outputs1[outputs1.length - 1], outputs2[outputs2.length - 1]))
  t.end()
})

test('`pnpm recursive run` fails when run without filters and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  let err!: PnpmError
  try {
    await recursive.handler(['run', 'this-command-does-not-exist'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` fails when run with a filter that includes all packages and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await recursive.handler(['run', 'this-command-does-not-exist'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      filter: ['*'],
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` succeeds when run against a subset of packages and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await recursive.handler(['run', 'this-command-does-not-exist'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    filter: ['project-1'],
  })
  t.end()
})

test('testing the bail config with "pnpm recursive run"', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
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
        build: `exit 1 && node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  let err1!: PnpmError
  try {
    await recursive.handler(['run', 'build', '--no-bail'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err1 = _err
  }
  t.equal(err1.code, 'ERR_PNPM_RECURSIVE_FAIL')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  let err2!: PnpmError
  try {
    await recursive.handler(['run', 'build'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err2 = _err
  }

  t.equal(err2.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.end()
})

test('pnpm recursive run with filtering', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output.json`,
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await recursive.handler(['run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    filter: ['project-1'],
  })

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
  t.end()
})

test('`pnpm recursive run` should always trust the scripts', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project')" | json-append ../output.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  process.env['npm_config_unsafe_perm'] = 'false'
  await recursive.handler(['run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  delete process.env['npm_config_unsafe_perm']

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project'])
  t.end()
})
