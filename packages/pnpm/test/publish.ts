import prepare, {
  prepareWithYamlManifest,
  prepareWithJson5Manifest,
} from '@pnpm/prepare'
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

const CREDENTIALS = [
  '--//localhost:4873/:username=username',
  `--//localhost:4873/:_password=${Buffer.from('password').toString('base64')}`,
  '--//localhost:4873/:email=foo@bar.net',
]

test('publish: package with package.json', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execPnpm('publish', ...CREDENTIALS)
})

test('publish: package with package.yaml', async (t: tape.Test) => {
  prepareWithYamlManifest(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5', async (t: tape.Test) => {
  prepareWithJson5Manifest(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5 running publish from different folder', async (t: tape.Test) => {
  prepareWithJson5Manifest(t, {
    name: 'test-publish-package.json5',
    version: '0.0.1',
  })

  process.chdir('..')

  await execPnpm('publish', 'project', ...CREDENTIALS)

  t.ok(await exists('project/package.json5'))
  t.notOk(await exists('project/package.json'))
})
