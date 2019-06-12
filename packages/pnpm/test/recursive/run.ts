import { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import rimraf = require('rimraf-then')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm recursive run', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')
  await execPnpm('recursive', 'run', 'build')

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
})

test('pnpm recursive run concurrently', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')
  await execPnpm('recursive', 'run', 'build')

  const outputs1 = await import(path.resolve('output1.json')) as number[]
  const outputs2 = await import(path.resolve('output2.json')) as number[]

  t.ok(Math.max(outputs1[0], outputs2[0]) < Math.min(outputs1[outputs1.length - 1], outputs2[outputs2.length - 1]))
})

test('`pnpm recursive run` fails if none of the packaegs has the desired command', async (t: tape.Test) => {
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
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  await execPnpm('recursive', 'install')

  try {
    await execPnpm('recursive', 'run', 'this-command-does-not-exist')
    t.fail('should have failed')
  } catch (err) {
    t.pass('`recursive run` failed because none of the packages has the wanted script')
  }
})

test('testing the bail config with "pnpm recursive run"', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  let failed = false
  try {
    await execPnpm('recursive', 'run', 'build', '--no-bail')
  } catch (err) {
    failed = true
  }
  t.ok(failed, 'recursive run failed with --no-bail')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  failed = false
  try {
    await execPnpm('recursive', 'run', 'build')
  } catch (err) {
    failed = true
  }

  t.ok(failed, 'recursive run failed with --bail')
})

test('pnpm recursive run with filtering', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')
  await execPnpm('run', 'build', '--filter', 'project-1')

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
})

test('`pnpm recursive run` should always trust the scripts', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  process.env['npm_config_unsafe_perm'] = 'false'
  await execPnpm('recursive', 'run', 'build')
  delete process.env['npm_config_unsafe_perm']

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project'])
})

test('pnpm recursive run finds bins from the root of the workspace', async (t: tape.Test) => {
  preparePackages(t, [
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
          'print-version': '2',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'print-version': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-build')" | json-append ../build-output.json`,
        postinstall: `node -e "process.stdout.write('project-postinstall')" | json-append ../postinstall-output.json`,
        testBinPriority: 'print-version | json-append ../testBinPriority.json',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm('recursive', 'install')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'))),
    ['project-postinstall'],
  )

  await execPnpm('recursive', 'run', 'build')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'))),
    ['project-build'],
  )

  process.chdir('project')
  await execPnpm('run', 'build')
  process.chdir('..')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'))),
    ['project-build', 'project-build'],
  )

  await execPnpm('recursive', 'rebuild')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'))),
    ['project-postinstall', 'project-postinstall'],
  )

  await execPnpm('recursive', 'run', 'testBinPriority')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('testBinPriority.json'))),
    ['1.0.0\n'],
  )
})
