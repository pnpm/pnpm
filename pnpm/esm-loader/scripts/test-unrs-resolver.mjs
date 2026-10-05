import assert from 'node:assert/strict'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { test } from 'node:test'
import { URL } from 'node:url'

import { fixture } from '../test/fixture.mjs'

const repoRequire = createRequire(new URL('../../../package.json', import.meta.url))
const jestRequire = createRequire(repoRequire.resolve('jest'))
const cliRequire = createRequire(jestRequire.resolve('jest-cli'))
const configRequire = createRequire(cliRequire.resolve('jest-config'))
const resolverRequire = createRequire(configRequire.resolve('jest-resolve'))
const unrs = path.dirname(resolverRequire.resolve('unrs-resolver'))

test('patched unrs uses Node hooks without a native binding or node_modules', context => {
  const setup = fixture(context)
  setup.add('unrs', Object.fromEntries(['index.js', 'node-resolution.cjs', 'package.json'].map(name => [name, fs.readFileSync(path.join(unrs, name))])))
  setup.add('example', {
    'package.json': JSON.stringify({ exports: { custom: './custom.js', require: './index.js' } }),
    'index.js': 'module.exports = 42', 'custom.js': 'module.exports = 7',
  })
  setup.manifest.packages['.'].dependencies = { 'unrs-resolver': 'unrs', example: 'example' }
  const result = setup.run(`
    import assert from 'node:assert/strict'
    import { createRequire } from 'node:module'
    import { fileURLToPath } from 'node:url'
    const require = createRequire(import.meta.url)
    const { ResolverFactory, sync } = require('unrs-resolver')
    const resolver = ResolverFactory.default()
    const normal = resolver.sync(process.cwd(), 'example')
    assert.equal(normal.path, require.resolve('example'))
    assert.deepEqual(await resolver.async(process.cwd(), 'example'), normal)
    assert.deepEqual(resolver.resolveFileSync(fileURLToPath(import.meta.url), 'example'), normal)
    assert.deepEqual(await resolver.resolveFileAsync(fileURLToPath(import.meta.url), 'example'), normal)
    assert.deepEqual(sync(process.cwd(), 'example'), normal)
    const custom = resolver.cloneWithOptions({ conditionNames: ['custom', 'node'] })
    assert.match(custom.sync(process.cwd(), 'example').path, /custom.js$/)
    assert.match(custom.sync(process.cwd(), 'missing').error, /missing/)
    assert.match(require.resolve('example'), /index.js$/)
    assert.deepEqual(resolver.sync(process.cwd(), 'node:fs').builtin, { resolved: 'node:fs', isRuntimeModule: true })
    resolver.clearCache()
    console.log('ok')
  `, { env: { UNRS_RESOLVER_NODE_RESOLUTION: '1' } })
  assert.equal(result.stdout.trim(), 'ok')
})
