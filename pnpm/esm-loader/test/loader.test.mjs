import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'

import { fixture } from './fixture.mjs'


const esm = JSON.stringify({ name: 'example', type: 'module', exports: './index.js' })

test('runs ESM and CommonJS from store blobs without materializing packages', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': esm,
    'index.js': "import cjs from 'legacy'; import { suffix } from './suffix.js'; export default cjs + suffix",
    'suffix.js': "export const suffix = ' world'",
  }, { legacy: 'legacy@1' })
  setup.add('legacy@1', {
    'package.json': '{"main":"index.cjs"}',
    'index.cjs': "module.exports = require('./value')",
    'value.js': "module.exports = 'hello'",
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(value)").stdout.trim(), 'hello world')
})

test('resolves CommonJS dot directories within the package and rejects parent escapes', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': '{"main":"index.cjs"}',
    'index.cjs': 'module.exports = 42',
    'bin/entry.cjs': "module.exports = [require('..'), require('.')];",
    'bin/index.js': 'module.exports = 7',
    'escape.cjs': "require('..')",
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example/bin/entry.cjs'; console.log(value.join(','))").stdout.trim(), '42,7')
  assert.match(setup.run("import 'example/escape.cjs'", { failure: true }).stderr, /ERR_PNPM_LOADER_PATH_ESCAPE/)
})

test('keeps distinct peer contexts even when every source blob is identical', context => {
  const setup = fixture(context)
  const sources = { 'package.json': esm, 'index.js': "export { default } from 'peer'" }
  setup.add('example@1(peer@1)', sources, { peer: 'peer@1' })
  setup.add('example@1(peer@2)', sources, { peer: 'peer@2' })
  for (const version of [1, 2]) setup.add(`peer@${version}`, { 'index.js': `module.exports = ${version}` })
  setup.manifest.packages['.'].dependencies = { first: 'example@1(peer@1)', second: 'example@1(peer@2)' }
  assert.equal(setup.run("import first from 'first'; import second from 'second'; console.log(first, second)").stdout.trim(), '1 2')
})

test('resolves conditional exports, private imports, self references, and patterns', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': JSON.stringify({
      name: 'example', type: 'module',
      exports: { '.': { custom: './custom.js', import: './index.js', require: './require.cjs' }, './features/*': './src/*.js', './blocked': null },
      imports: { '#value': './value.js', '#dep': 'external' },
    }),
    'index.js': "import value from '#value'; import { feature } from 'example/features/test'; import dep from '#dep'; export default value + feature + dep",
    'custom.js': 'export default 99',
    'require.cjs': 'module.exports = 12',
    'src/test.js': 'export const feature = 2',
    'value.js': 'export default 1',
  }, { external: 'external@1' })
  setup.add('external@1', { 'index.js': 'module.exports = 3' })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  const source = "import value from 'example'; import { createRequire } from 'node:module'; console.log(value, createRequire(import.meta.url)('example'))"
  assert.equal(setup.run(source).stdout.trim(), '6 12')
  assert.equal(setup.run(source, { args: ['--conditions=custom'] }).stdout.trim(), '99 [Module: null prototype] { __esModule: true, default: 99 }')
  assert.match(setup.run("import 'example/blocked'", { failure: true }).stderr, /not exported/)
})

test('loads JSON, nested package types, builtins and dynamic imports', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': esm,
    'index.js': "import data from './data.json' with { type: 'json' }; import cjs from './nested/value.js'; import { basename } from 'node:path'; export default [data.value, cjs, basename('/a/b'), (await import('./dynamic.mjs')).default]",
    'data.json': '{"value":1}',
    'nested/package.json': '{"type":"commonjs"}',
    'nested/value.js': 'module.exports = require("../data.json").value + 1',
    'dynamic.mjs': 'export default 3',
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(JSON.stringify(value))").stdout.trim(), '[1,2,"b",3]')
})

test('enforces declared dependencies, exports, and ESM file extensions', context => {
  const setup = fixture(context)
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  const files = { 'package.json': esm, 'index.js': "import 'undeclared'", 'private.js': 'export default 1' }
  setup.add('example@1', files)
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_UNDECLARED_DEPENDENCY/)
  assert.match(setup.run("import 'example/private.js'", { failure: true }).stderr, /not exported/)
  setup.add('example@1', { ...files, 'index.js': "import './private'" })
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /resolve/)
})

test('verifies store integrity before executing a module', context => {
  const setup = fixture(context)
  const files = setup.add('example@1', { 'package.json': esm, 'index.js': 'export default 1' })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  const hash = files['index.js']
  setup.write(`store/files/${hash.slice(0, 2)}/${hash.slice(2)}`, "throw new Error('tampered source ran')")
  const result = setup.run("import 'example'", { failure: true })
  assert.match(result.stderr, /ERR_PNPM_LOADER_INTEGRITY/)
  assert.doesNotMatch(result.stderr, /tampered source ran/)
})

test('rejects traversal in manifests and module requests', context => {
  const setup = fixture(context)
  const files = setup.add('example@1', { 'package.json': esm, 'index.js': "import '../../app.mjs'" })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_PATH_ESCAPE/)
  setup.manifest.packages['example@1'].files['../escape.js'] = files['index.js']
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_MANIFEST/)
})

test('does not intercept files outside registered project roots', context => {
  const setup = fixture(context)
  setup.manifest.packages['.'].root = './project'
  assert.equal(setup.run("import path from 'node:path'; console.log(path.basename('/a/b'))").stdout.trim(), 'b')
})

test('preserves URL query identity and encoded filenames', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': esm,
    'index.js': "import first from './value%20%23.js?first'; import same from './value%20%23.js?first'; import other from './value%20%23.js?other'; export default [first === same, first !== other]",
    'value #.js': 'export default {}',
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(JSON.stringify(value))").stdout.trim(), '[true,true]')
})

test('enforces JSON import attributes while allowing require and resolution of JSON', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': JSON.stringify({ type: 'module', exports: { '.': './data.json', './code': './index.js' } }),
    'data.json': '{"value":42}', 'index.js': 'export default 1',
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.match(setup.run("console.log(import.meta.resolve('example'))").stdout, /data.json/)
  assert.match(setup.run("import value from 'example'", { failure: true }).stderr, /ERR_IMPORT_ATTRIBUTE_MISSING/)
  assert.match(setup.run("import value from 'example/code' with { type: 'json' }", { failure: true }).stderr, /ERR_IMPORT_ATTRIBUTE_TYPE_INCOMPATIBLE/)
  assert.equal(setup.run("import { createRequire } from 'node:module'; console.log(createRequire(import.meta.url)('example').value)").stdout.trim(), '42')
})

test('supports CommonJS entry points, cycles, and createRequire inside store modules', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'package.json': '{"type":"module","exports":"./index.js"}',
    'index.js': "import { createRequire } from 'node:module'; export default createRequire(import.meta.url)('./first.cjs')",
    'first.cjs': "exports.value = 1; exports.next = require('./second.cjs').value",
    'second.cjs': "exports.value = require('./first.cjs').value + 1",
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  setup.write('entry.cjs', "import('example').then(({ default: value }) => console.log(JSON.stringify(value)))")
  assert.equal(setup.run("import './entry.cjs'").stdout.trim(), '{"value":1,"next":2}')
})

test('inherits registration in workers and routes workspace dependency contexts', context => {
  const setup = fixture(context)
  setup.manifest.packages.workspace = { root: './workspace', dependencies: { example: 'example@1' } }
  setup.add('example@1', { 'index.js': 'module.exports = 42' })
  setup.write('workspace/worker.mjs', "import value from 'example'; import { parentPort } from 'node:worker_threads'; parentPort.postMessage(value)")
  assert.equal(setup.run("import { Worker } from 'node:worker_threads'; const worker = new Worker(new URL('./workspace/worker.mjs', import.meta.url)); worker.on('message', value => console.log(value))").stdout.trim(), '42')
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_UNDECLARED_DEPENDENCY/)
})

test('rejects unsupported native addons explicitly', context => {
  const setup = fixture(context)
  setup.add('addon@1', { 'package.json': '{"main":"binding.node"}', 'binding.node': 'not a native binary' })
  setup.manifest.packages['.'].dependencies.addon = 'addon@1'
  assert.match(setup.run("import 'addon'", { failure: true }).stderr, /ERR_PNPM_LOADER_UNSUPPORTED_FORMAT/)
  assert.match(setup.run("import { createRequire } from 'node:module'; createRequire(import.meta.url)('addon')", { failure: true }).stderr, /ERR_PNPM_LOADER_UNSUPPORTED_FORMAT/)
})

test('handles executable store blobs, missing files, and unknown dependency targets', context => {
  const setup = fixture(context)
  const files = setup.add('example@1', { 'package.json': esm, 'index.js': '#!/usr/bin/env node\nexport default 42' })
  const hash = files['index.js']
  const blob = `store/files/${hash.slice(0, 2)}/${hash.slice(2)}`
  fs.renameSync(path.join(setup.root, blob), path.join(setup.root, `${blob}-exec`))
  files['index.js'] += '-exec'
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(value)").stdout.trim(), '42')
  fs.unlinkSync(path.join(setup.root, `${blob}-exec`))
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ENOENT/)
  setup.manifest.packages['.'].dependencies.missing = 'missing@1'
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_MANIFEST/)
})

test('preserves literal CommonJS filenames and require.resolve results', context => {
  const setup = fixture(context)
  setup.add('example@1', {
    'index.js': "module.exports = require(require.resolve('./value #.cjs'))",
    'value #.cjs': 'module.exports = 42',
  })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(value)").stdout.trim(), '42')
})

test('rejects malformed manifests before executing application code', context => {
  const setup = fixture(context)
  for (const invalid of [null, [], 'invalid', { root: 1 }, { files: null }, { root: '.', files: {} }, { root: '.', dependencies: [] }, { root: '.', resolution: 'node' }]) {
    setup.manifest.packages.invalid = invalid
    assert.match(setup.run("throw new Error('application ran')", { failure: true }).stderr, /ERR_PNPM_LOADER_MANIFEST/)
  }
})

test('does not resolve inherited properties as dependency declarations', context => {
  const setup = fixture(context)
  setup.add('example@1', { 'index.js': "require('toString')" })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.match(setup.run("import 'example'", { failure: true }).stderr, /ERR_PNPM_LOADER_UNDECLARED_DEPENDENCY/)
})

test('rejects package subpaths that escape into another dependency context', context => {
  const setup = fixture(context)
  setup.add('example@1', { 'index.js': 'module.exports = 1' })
  setup.add('hidden@1', { 'index.js': 'module.exports = 2' })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  const hiddenRoot = createHash('sha256').update('hidden@1').digest('hex')
  assert.match(setup.run(`import 'example/../${hiddenRoot}/index.js'`, { failure: true }).stderr, /ERR_PNPM_LOADER_PATH_ESCAPE/)
})

test('loads resolved absolute paths from a stored tool', context => {
  const setup = fixture(context)
  setup.add('tool@1', { 'index.js': 'exports.load = filename => require(filename)' })
  setup.add('value@1', { 'index.js': 'module.exports = 42' })
  setup.manifest.packages['.'].dependencies = { tool: 'tool@1', value: 'value@1' }
  setup.write('config.cjs', 'module.exports = 7')
  assert.equal(setup.run("import { createRequire } from 'node:module'; const require = createRequire(import.meta.url); const tool = require('tool'); console.log(tool.load(require.resolve('value')), tool.load(require.resolve('./config.cjs')))").stdout.trim(), '42 7')
})

test('reads package types from manifests with a UTF-8 BOM', context => {
  const setup = fixture(context)
  setup.add('example@1', { 'package.json': '\uFEFF' + esm, 'index.js': 'export default 42' })
  setup.manifest.packages['.'].dependencies.example = 'example@1'
  assert.equal(setup.run("import value from 'example'; console.log(value)").stdout.trim(), '42')
})

test('runs opted-out packages and their linked dependencies with native resolution', context => {
  const setup = fixture(context)
  setup.manifest.packages['.'].root = './project'
  setup.manifest.packages['.'].dependencies = { tool: 'tool@1', stored: 'stored@1' }
  setup.add('stored@1', { 'index.js': 'module.exports = "CAS"' })
  for (const version of [1, 2]) {
    const tool = `gvs/tool-${version}/node_modules/tool`
    const dependency = `gvs/value-${version}/node_modules/value`
    setup.write(`${tool}/package.json`, '{"name":"tool","main":"index.cjs"}')
    setup.write(`${tool}/index.cjs`, `
      const fs = require('node:fs')
      const path = require('node:path')
      const { execFileSync } = require('node:child_process')
      exports.value = require('value')
      exports.asset = fs.readFileSync(path.join(path.dirname(require.resolve('value')), 'asset.txt'), 'utf8')
      exports.child = execFileSync(process.execPath, ['-e', 'console.log(require("value"))'], {
        cwd: __dirname, env: { ...process.env, NODE_OPTIONS: '' }, encoding: 'utf8',
      }).trim()
    `)
    setup.write(`${dependency}/package.json`, '{"name":"value","main":"index.cjs"}')
    setup.write(`${dependency}/index.cjs`, `module.exports = ${version}`)
    setup.write(`${dependency}/asset.txt`, `asset ${version}`)
    fs.symlinkSync(path.join(setup.root, dependency), path.join(setup.root, `gvs/tool-${version}/node_modules/value`), 'junction')
    setup.manifest.packages[`tool@${version}`] = { root: tool, resolution: 'node' }
    setup.manifest.packages[`value@${version}`] = { root: dependency, resolution: 'node' }
  }
  setup.manifest.packages['.'].dependencies.other = 'tool@2'
  setup.manifest.packages.alias = { ...setup.manifest.packages['tool@1'] }
  setup.manifest.packages['.'].dependencies.alias = 'alias'
  setup.write('project/main.mjs', `
    import assert from 'node:assert/strict'
    import { createRequire } from 'node:module'
    import { fileURLToPath } from 'node:url'
    import first from 'tool'
    import second from 'other'
    import alias from 'alias'
    import stored from 'stored'
    assert.equal(first, alias)
    assert.deepEqual(first, { value: 1, asset: 'asset 1', child: '1' })
    assert.deepEqual(second, { value: 2, asset: 'asset 2', child: '2' })
    assert.equal(stored, 'CAS')
    assert.equal(createRequire(import.meta.resolve('tool'))(fileURLToPath(import.meta.resolve('stored'))), 'CAS')
    assert.equal(await import.meta.resolve('tool'), new URL('../gvs/tool-1/node_modules/tool/index.cjs', import.meta.url).href)
  `)
  setup.run("await import('./project/main.mjs')")
  assert.equal(fs.existsSync(path.join(setup.root, 'project/node_modules')), false)
})

test('rejects native resolution for stored packages and unknown resolution modes', context => {
  const setup = fixture(context)
  setup.add('example@1', { 'index.js': 'module.exports = 1' })
  setup.manifest.packages['example@1'].resolution = 'node'
  assert.match(setup.run('', { failure: true }).stderr, /ERR_PNPM_LOADER_MANIFEST/)
  setup.manifest.packages['example@1'] = { root: './physical', resolution: 'unknown' }
  assert.match(setup.run('', { failure: true }).stderr, /ERR_PNPM_LOADER_MANIFEST/)
})
