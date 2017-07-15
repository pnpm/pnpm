import test = require('tape')
import list, {forPackages as listForPackages} from '../src'
import path = require('path')
import {stripIndent} from 'common-tags'
import chalk = require('chalk')

const highlighted = chalk.yellow.bgBlack

const fixture = path.join(__dirname, 'fixture')

test('list with default parameters', async t => {
  t.equal(await list(fixture), stripIndent`
    test@1.0.0 ${fixture}
    ├── write-json-file@2.2.0
    ├── is-positive@3.1.0
    └── is-negative@2.1.0
  ` + '\n')

  t.end()
})

test('list with depth 1', async t => {
  t.equal(await list(fixture, {depth: 1}), stripIndent`
    test@1.0.0 ${fixture}
    ├─┬ write-json-file@2.2.0
    │ ├── detect-indent@5.0.0
    │ ├── graceful-fs@4.1.11
    │ ├── make-dir@1.0.0
    │ ├── pify@2.3.0
    │ ├── sort-keys@1.1.2
    │ └── write-file-atomic@2.1.0
    ├── is-positive@3.1.0
    └── is-negative@2.1.0
  ` + '\n')

  t.end()
})

test('list with depth 1 and selected packages', async t => {
  t.equal(await listForPackages(['make-dir', 'pify@2', 'sort-keys@2', 'is-negative'], fixture, {depth: 1}), stripIndent`
    test@1.0.0 ${fixture}
    ├─┬ write-json-file@2.2.0
    │ ├── ${highlighted('make-dir@1.0.0')}
    │ └── ${highlighted('pify@2.3.0')}
    └── ${highlighted('is-negative@2.1.0')}
  ` + '\n')

  t.end()
})

test('list in long format', async t => {
  t.equal(await list(fixture, {long: true}), stripIndent`
    test@1.0.0 ${fixture}
    ├── write-json-file@2.2.0
    │   Stringify and write JSON to a file atomically
    │   git+https://github.com/sindresorhus/write-json-file.git
    │   https://github.com/sindresorhus/write-json-file#readme
    ├── is-positive@3.1.0
    │   Check if something is a positive number
    │   git+https://github.com/kevva/is-positive.git
    │   https://github.com/kevva/is-positive#readme
    └── is-negative@2.1.0
        Check if something is a negative number
        git+https://github.com/kevva/is-negative.git
        https://github.com/kevva/is-negative#readme
  ` + '\n')

  t.end()
})
