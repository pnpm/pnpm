import prepare from '@pnpm/prepare'
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, pathToLocalPkg } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('prune removes external link that is not in package.json', async function (t: tape.Test) {
  const project = prepare(t)

  await execPnpm('link', path.relative(process.cwd(), pathToLocalPkg('local-pkg')))

  await project.has('local-pkg')

  await execPnpm('prune')

  await project.hasNot('local-pkg')
})
