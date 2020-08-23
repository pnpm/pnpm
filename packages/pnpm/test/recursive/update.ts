import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'
import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')

const test = promisifyTape(tape)

// TODO: This should work if the settings are passed through CLI
test.skip('recursive update --latest should update deps with correct specs', async (t: tape.Test) => {
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
  ])

  await fs.writeFile(
    'project-2/.npmrc',
    'save-exact = true',
    'utf8'
  )

  await fs.writeFile(
    'project-3/.npmrc',
    'save-prefix = ~',
    'utf8'
  )

  await execPnpm(['recursive', 'update', '--latest'])

  t.deepEqual((await import(path.resolve('project-1/package.json'))).dependencies, { foo: '^100.1.0' })
  t.deepEqual((await import(path.resolve('project-2/package.json'))).dependencies, { foo: '100.1.0' })
  t.deepEqual((await import(path.resolve('project-3/package.json'))).dependencies, { foo: '~100.1.0' })
})
