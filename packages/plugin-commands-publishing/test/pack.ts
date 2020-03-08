import { pack } from '@pnpm/plugin-commands-publishing'
import prepare from '@pnpm/prepare'
import exists = require('path-exists')
import test = require('tape')

test('pack: package with package.json', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  t.ok(await exists('test-publish-package.json-0.0.0.tgz'))
  t.ok(await exists('package.json'))
  t.end()
})

test('pack: package with package.yaml', async (t) => {
  prepare(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  t.ok(await exists('test-publish-package.yaml-0.0.0.tgz'))
  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
  t.end()
})

test('pack: package with package.json5', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  t.ok(await exists('test-publish-package.json5-0.0.0.tgz'))
  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
  t.end()
})
