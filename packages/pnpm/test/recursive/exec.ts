import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import rimraf = require('rimraf-then')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm recursive exec', async (t: tape.Test) => {
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
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'exec', 'npm', 'run', 'build')

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
})

test('pnpm recursive exec sets PNPM_PACKAGE_NAME env var', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])

  const result = execPnpmSync('recursive', 'exec', '--', 'node', '-e', 'process.stdout.write(process.env.PNPM_PACKAGE_NAME)')

  t.equal(result.stdout.toString(), 'foo', '$PNPM_PACKAGE_NAME is correct')
})

test('testing the bail config with "pnpm recursive exec"', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'link')

  let failed = false
  try {
    await execPnpm('recursive', 'exec', 'npm', 'run', 'build', '--no-bail')
  } catch (err) {
    failed = true
  }
  t.ok(failed, 'recursive exec failed with --no-bail')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  failed = false
  try {
    await execPnpm('recursive', 'exec', 'npm', 'run', 'build')
  } catch (err) {
    failed = true
  }

  t.ok(failed, 'recursive exec failed with --bail')
})

test('pnpm recursive exec --no-sort', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'a-dependent',
      version: '1.0.0',

      dependencies: {
        'b-dependency': '1.0.0',
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('a-dependent')" | json-append ../output.json`,
      },
    },
    {
      name: 'b-dependency',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('b-dependency')" | json-append ../output.json`,
      },
    },
  ])

  await execPnpm('recursive', 'install', '--link-workspace-packages')
  await execPnpm('recursive', 'exec', 'npm', 'run', 'build', '--no-sort', '--workspace-concurrency', '1')

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['a-dependent', 'b-dependency'])
})
