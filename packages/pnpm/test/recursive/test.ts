import { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm recursive test', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
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
        test: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
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
        test: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'test')

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
})

test('`pnpm recursive test` does not fail if none of the packaegs has a test command', async (t: tape.Test) => {
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

      dependencies: {},
    },
  ])

  await execPnpm('recursive', 'link')

  await execPnpm('recursive', 'test')

  t.pass('command did not fail')
})

test('pnpm recursive test with filtering', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
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
        test: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
  ])

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  await execPnpm('recursive', 'install')
  await execPnpm('test', '--filter', 'project-1')

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
})
