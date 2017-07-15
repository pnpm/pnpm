import test = require('tape')
import pnpmList from '../src'
import path = require('path')
import {stripIndent} from 'common-tags'

const fixture = path.join(__dirname, 'fixture')

test('list with default parameters', async t => {
  t.equal(await pnpmList(fixture, [], {}), stripIndent`
    test@1.0.0 ${fixture}
    ├── write-json-file@2.2.0
    ├── is-positive@3.1.0
    └── is-negative@2.1.0
  ` + '\n')

  t.end()
})

test('list with depth 1', async t => {
  t.equal(await pnpmList(fixture, [], {depth: 1}), stripIndent`
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
