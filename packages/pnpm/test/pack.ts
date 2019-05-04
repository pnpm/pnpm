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

test('pack: package with package.json', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execPnpm('pack')

  t.ok(await exists('test-publish-package.json-0.0.0.tgz'))
  t.ok(await exists('package.json'))
})

test('pack: package with package.yaml', async (t: tape.Test) => {
  prepareWithYamlManifest(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  })

  await execPnpm('pack')

  t.ok(await exists('test-publish-package.yaml-0.0.0.tgz'))
  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
})

test('pack: package with package.json5', async (t: tape.Test) => {
  prepareWithJson5Manifest(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  })

  await execPnpm('pack')

  t.ok(await exists('test-publish-package.json5-0.0.0.tgz'))
  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
})
