import {stripIndents} from 'common-tags'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
  preparePackages,
} from '../utils'
import normalizeNewline = require('normalize-newline')

const test = promisifyTape(tape)

test('pnpm recursive outdated', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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

  {
    const result = execPnpmSync('recursive', 'outdated')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), '           ' + stripIndents`
                Package      Current  Wanted  Latest
      project-1  is-positive  1.0.0    1.0.0   3.1.0
      project-2  is-negative  1.0.0    1.0.0   2.1.0
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', 'is-positive')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), '           ' + stripIndents`
                Package      Current  Wanted  Latest
      project-1  is-positive  1.0.0    1.0.0   3.1.0
    ` + '\n')
  }
})
