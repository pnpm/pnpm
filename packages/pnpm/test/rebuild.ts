import prepare from '@pnpm/prepare'
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

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

// Covers https://github.com/pnpm/pnpm/issues/1969
test('rebuild a package with no deps when independent-leaves is true', async (t: tape.Test) => {
  prepare(t)
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm('add', 'independent-and-requires-build@1.0.0', '-W', '--no-hoist', '--independent-leaves', '--link-workspace-packages')

  t.ok(await exists(path.resolve('node_modules/independent-and-requires-build/created-by-postinstall')))
})
