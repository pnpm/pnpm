import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {stripIndent} from 'common-tags'
import {
  execPnpm,
  execPnpmSync,
  prepare,
} from './utils'

const test = promisifyTape(tape)

test('rebuild', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', '--ignore-scripts', 'pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example')

  await execPnpm('rebuild')

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

test('rebuilds specific dependencies', async function (t: tape.Test) {
  const project = prepare(t)

  await execPnpm('install', '--ignore-scripts', 'pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example')

  await execPnpm('rebuild', 'install-scripts-example-for-pnpm')

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})
