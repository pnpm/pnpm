import { execPnpm } from '../utils'
import { preparePackages } from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import fs = require('mz/fs')
import path = require('path')
import tape = require('tape')
import writeYamlFile = require('write-yaml-file')

const test = promisifyTape(tape)

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
        build: 'node -e "process.stdout.write(\'project-build\')" | json-append ../build-output.json',
        postinstall: 'node -e "process.stdout.write(\'project-postinstall\')" | json-append ../postinstall-output.json',
        testBinPriority: 'print-version | json-append ../testBinPriority.json',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['recursive', 'install'])

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'), 'utf8')),
    ['project-postinstall']
  )

  await execPnpm(['recursive', 'run', 'build'])

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'), 'utf8')),
    ['project-build']
  )

  process.chdir('project')
  await execPnpm(['run', 'build'])
  process.chdir('..')

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('build-output.json'), 'utf8')),
    ['project-build', 'project-build']
  )

  await execPnpm(['recursive', 'rebuild'])

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('postinstall-output.json'), 'utf8')),
    ['project-postinstall', 'project-postinstall']
  )

  await execPnpm(['recursive', 'run', 'testBinPriority'])

  t.deepEqual(
    JSON.parse(await fs.readFile(path.resolve('testBinPriority.json'), 'utf8')),
    ['1.0.0\n']
  )
})
