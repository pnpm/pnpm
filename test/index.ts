import test = require('tape')
import list, {forPackages as listForPackages} from 'pnpm-list'
import path = require('path')
import {stripIndent} from 'common-tags'
import chalk from 'chalk'

const highlighted = chalk.yellow.bgBlack

const fixture = path.join(__dirname, 'fixture')
const emptyFixture = path.join(__dirname, 'empty')

test('list with default parameters', async t => {
  t.equal(await list(fixture), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └── write-json-file@2.2.0
  ` + '\n')

  t.end()
})

test('list dev only', async t => {
  t.equal(await list(fixture, {only: 'dev'}), stripIndent`
    fixture@1.0.0 ${fixture}
    └── is-positive@3.1.0
  ` + '\n')

  t.end()
})

test('list prod only', async t => {
  t.equal(await list(fixture, {only: 'prod'}), stripIndent`
    fixture@1.0.0 ${fixture}
    └── write-json-file@2.2.0
  ` + '\n')

  t.end()
})

test('list prod only with depth 2', async t => {
  t.equal(await list(fixture, {only: 'prod', depth: 2}), stripIndent`
    fixture@1.0.0 ${fixture}
    └─┬ write-json-file@2.2.0
      ├── detect-indent@5.0.0
      ├── graceful-fs@4.1.11
      ├─┬ make-dir@1.0.0
      │ └── pify@2.3.0
      ├── pify@2.3.0
      ├─┬ sort-keys@1.1.2
      │ └── is-plain-obj@1.1.0
      └─┬ write-file-atomic@2.1.0
        ├── graceful-fs@4.1.11
        ├── imurmurhash@0.1.4
        └── slide@1.1.6
  ` + '\n')

  t.end()
})

test('list with depth 1', async t => {
  t.equal(await list(fixture, {depth: 1}), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └─┬ write-json-file@2.2.0
      ├── detect-indent@5.0.0
      ├── graceful-fs@4.1.11
      ├── make-dir@1.0.0
      ├── pify@2.3.0
      ├── sort-keys@1.1.2
      └── write-file-atomic@2.1.0
  ` + '\n')

  t.end()
})

test('list with depth 1 and selected packages', async t => {
  t.equal(await listForPackages(['make-dir', 'pify@2', 'sort-keys@2', 'is-negative'], fixture, {depth: 1}), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── ${highlighted('is-negative@2.1.0')}
    └─┬ write-json-file@2.2.0
      ├── ${highlighted('make-dir@1.0.0')}
      └── ${highlighted('pify@2.3.0')}
  ` + '\n')

  t.end()
})

test('list in long format', async t => {
  t.equal(await list(fixture, {long: true}), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    │   Check if something is a negative number
    │   git+https://github.com/kevva/is-negative.git
    │   https://github.com/kevva/is-negative#readme
    ├── is-positive@3.1.0
    │   Check if something is a positive number
    │   git+https://github.com/kevva/is-positive.git
    │   https://github.com/kevva/is-positive#readme
    └── write-json-file@2.2.0
        Stringify and write JSON to a file atomically
        git+https://github.com/sindresorhus/write-json-file.git
        https://github.com/sindresorhus/write-json-file#readme
  ` + '\n')

  t.end()
})

test('parseable list with depth 1', async t => {
  t.equal(await list(fixture, {parseable: true, depth: 1}), stripIndent`
    ${fixture}
    ${fixture}/node_modules/.registry.npmjs.org/detect-indent/5.0.0
    ${fixture}/node_modules/.registry.npmjs.org/graceful-fs/4.1.11
    ${fixture}/node_modules/.registry.npmjs.org/is-negative/2.1.0
    ${fixture}/node_modules/.registry.npmjs.org/is-positive/3.1.0
    ${fixture}/node_modules/.registry.npmjs.org/make-dir/1.0.0
    ${fixture}/node_modules/.registry.npmjs.org/pify/2.3.0
    ${fixture}/node_modules/.registry.npmjs.org/sort-keys/1.1.2
    ${fixture}/node_modules/.registry.npmjs.org/write-file-atomic/2.1.0
    ${fixture}/node_modules/.registry.npmjs.org/write-json-file/2.2.0
    ` + '\n')

  t.end()
})

test('parseable list with depth 1 and dev only', async t => {
  t.equal(await list(fixture, {parseable: true, depth: 1, only: 'dev'}), stripIndent`
    ${fixture}
    ${fixture}/node_modules/.registry.npmjs.org/is-positive/3.1.0
  ` + '\n')

  t.end()
})

test('long parseable list with depth 1', async t => {
  t.equal(await list(fixture, {parseable: true, depth: 1, long: true}), stripIndent`
    ${fixture}:fixture@1.0.0
    ${fixture}/node_modules/.registry.npmjs.org/detect-indent/5.0.0:detect-indent@5.0.0
    ${fixture}/node_modules/.registry.npmjs.org/graceful-fs/4.1.11:graceful-fs@4.1.11
    ${fixture}/node_modules/.registry.npmjs.org/is-negative/2.1.0:is-negative@2.1.0
    ${fixture}/node_modules/.registry.npmjs.org/is-positive/3.1.0:is-positive@3.1.0
    ${fixture}/node_modules/.registry.npmjs.org/make-dir/1.0.0:make-dir@1.0.0
    ${fixture}/node_modules/.registry.npmjs.org/pify/2.3.0:pify@2.3.0
    ${fixture}/node_modules/.registry.npmjs.org/sort-keys/1.1.2:sort-keys@1.1.2
    ${fixture}/node_modules/.registry.npmjs.org/write-file-atomic/2.1.0:write-file-atomic@2.1.0
    ${fixture}/node_modules/.registry.npmjs.org/write-json-file/2.2.0:write-json-file@2.2.0
  ` + '\n')

  t.end()
})

test('print empty', async t => {
  t.equal(await list(emptyFixture), `empty@1.0.0 ${emptyFixture}\n`)
  t.end()
})

test("don't print empty", async t => {
  t.equal(await list(emptyFixture, {alwaysPrintRootPackage: false}), '')
  t.end()
})
