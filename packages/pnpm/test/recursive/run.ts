import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {
  execPnpm,
  preparePackages,
} from '../utils'

const test = promisifyTape(tape)

test('pnpm recursive run', async (t: tape.Test) => {
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
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output.json`,
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output.json`,
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
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')
  await execPnpm('recursive', 'run', 'build')

  const outputs = await import(path.resolve('output.json')) as string[]
  const p1 = outputs.indexOf('project-1')
  const p2 = outputs.indexOf('project-2')
  const p2pre = outputs.indexOf('project-2-prebuild')
  const p2post = outputs.indexOf('project-2-postbuild')
  const p3 = outputs.indexOf('project-3')

  t.ok(p1 < p2 && p1 < p3)
  t.ok(p1 < p2pre && p1 < p2post)
  t.ok(p2 < p2post && p2 > p2pre)
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
      dependencies: {
      },
    },
  ])

  await execPnpm('recursive', 'link')

  try {
    await execPnpm('recursive', 'run', 'this-command-does-not-exist')
    t.fail('should have failed')
  } catch (err) {
    t.pass('`recursive run` failed because none of the packages has the wanted script')
  }
})
