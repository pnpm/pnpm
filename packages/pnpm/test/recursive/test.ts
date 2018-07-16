import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {
  execPnpm,
  preparePackages,
} from '../utils'

const test = promisifyTape(tape)

test('pnpm recursive test', async (t: tape.Test) => {
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
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'test')

  const outputs = await import(path.resolve('output.json')) as string[]

  const p1 = outputs.indexOf('project-1')
  const p2 = outputs.indexOf('project-2')
  const p3 = outputs.indexOf('project-3')

  t.ok(p1 < p2 && p1 < p3)
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
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')

  await execPnpm('recursive', 'test')

  t.pass('command did not fail')
})
