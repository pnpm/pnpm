import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

import { importIndexedDir } from '../src/importIndexedDir.js'

test('importIndexedDir() keepModulesDir merges node_modules', async () => {
  const tmp = tempDir()
  fs.mkdirSync(path.join(tmp, 'src/node_modules/a'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'src/node_modules/a/index.js'), 'module.exports = 1')
  fs.mkdirSync(path.join(tmp, 'src/node_modules/@scope/bundled'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'src/node_modules/@scope/bundled/index.js'), 'module.exports = "bundled"')

  fs.mkdirSync(path.join(tmp, 'dest/node_modules/b'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'dest/node_modules/b/index.js'), 'module.exports = 1')
  fs.mkdirSync(path.join(tmp, 'dest/node_modules/@scope/preserved'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'dest/node_modules/@scope/preserved/index.js'), 'module.exports = "preserved"')
  fs.mkdirSync(path.join(tmp, 'dest/node_modules/@scope/bundled'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'dest/node_modules/@scope/bundled/index.js'), 'module.exports = "stale"')

  const newDir = path.join(tmp, 'dest')
  const filenames = new Map([
    ['node_modules/a/index.js', path.join(tmp, 'src/node_modules/a/index.js')],
    ['node_modules/@scope/bundled/index.js', path.join(tmp, 'src/node_modules/@scope/bundled/index.js')],
  ])
  importIndexedDir({ importFile: fs.linkSync, importFileAtomic: fs.linkSync }, newDir, filenames, { keepModulesDir: true })

  expect(fs.readdirSync(path.join(newDir, 'node_modules')).sort()).toEqual(['@scope', 'a', 'b'])
  expect(fs.readdirSync(path.join(newDir, 'node_modules/@scope')).sort()).toEqual(['bundled', 'preserved'])
  expect(fs.readFileSync(path.join(newDir, 'node_modules/@scope/bundled/index.js'), 'utf8')).toBe('module.exports = "bundled"')
  expect(fs.readFileSync(path.join(newDir, 'node_modules/@scope/preserved/index.js'), 'utf8')).toBe('module.exports = "preserved"')
})
