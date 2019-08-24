import { preparePackages } from '@pnpm/prepare'
import { stripIndents } from 'common-tags'
import normalizeNewline = require('normalize-newline')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm recursive outdated', async (t: tape.Test) => {
  preparePackages(t, [
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
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  {
    const result = execPnpmSync('recursive', 'outdated')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
      Package      Current  Wanted  Latest  Belongs To       Dependents
      is-negative  1.0.0    1.0.0   2.1.0   dependencies     project-2
      is-negative  1.0.0    1.0.0   2.1.0   devDependencies  project-3
      is-positive  1.0.0    1.0.0   3.1.0   dependencies     project-1, project-3
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', 'is-positive')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndents`
      Package      Current  Wanted  Latest  Belongs To    Dependents
      is-positive  1.0.0    1.0.0   3.1.0   dependencies  project-1, project-3
    ` + '\n')
  }
})
