import { pack } from '@pnpm/plugin-commands-publishing'
import prepare from '@pnpm/prepare'
import exists = require('path-exists')

test('pack: package with package.json', async () => {
  prepare(undefined, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  expect(await exists('test-publish-package.json-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.json')).toBeTruthy()
})

test('pack: package with package.yaml', async () => {
  prepare(undefined, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  expect(await exists('test-publish-package.yaml-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.yaml')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
})

test('pack: package with package.json5', async () => {
  prepare(undefined, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await pack.handler({ argv: { original: [] }, dir: process.cwd() })

  expect(await exists('test-publish-package.json5-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.json5')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
})
