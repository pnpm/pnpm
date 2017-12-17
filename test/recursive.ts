import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  execPnpm,
} from './utils'

const test = promisifyTape(tape)

test('recursive installation', async t => {
  const projects = prepare(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  t.end()
})
