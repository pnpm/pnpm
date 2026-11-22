import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { safeExeca as execa } from 'execa'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('PnP run and exec use lockfileDir for root and workspace projects', async () => {
  prepareEmpty()
  fs.mkdirSync('local-package')
  fs.writeFileSync('local-package/package.json', JSON.stringify({
    name: 'local-package', version: '1.0.0', exports: { '.': './entry.cjs', './sub': './sub.cjs' },
  }))
  fs.writeFileSync('local-package/entry.cjs', 'module.exports = "root"')
  fs.writeFileSync('local-package/sub.cjs', 'module.exports = "sub"')
  fs.writeFileSync('local-package/private.cjs', '')
  for (const [dir, dependency] of [['.', 'file:./local-package'], ['packages/member', 'file:../../local-package']]) {
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
      name: dir === '.' ? 'root' : 'member',
      dependencies: { 'local-package': dependency },
      scripts: { check: 'node check.cjs' },
    }))
    fs.writeFileSync(path.join(dir, 'check.cjs'), `
const assert = require('assert');
assert.strictEqual(require('local-package'), 'root');
assert.strictEqual(require('local-package/sub'), 'sub');
assert.throws(() => require('local-package/private.cjs'), {code: 'ERR_PACKAGE_PATH_NOT_EXPORTED'});
`)
  }
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\nnodeLinker: pnp\nsymlink: false\nlockfileDir: ./lockfiles\n')
  await execa(pnpmBin, ['install'])
  expect(fs.existsSync('lockfiles/.pnp.cjs')).toBe(true)
  expect(fs.existsSync('.pnp.cjs')).toBe(false)
  for (const args of [['run', 'check'], ['exec', 'node', 'check.cjs'], ['-r', 'run', 'check'], ['-r', 'exec', 'node', 'check.cjs']]) {
    // eslint-disable-next-line no-await-in-loop -- commands share task state in the same workspace
    await execa(pnpmBin, args)
  }
})
