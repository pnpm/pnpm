import { tempDir } from '@pnpm/prepare'
import fs from 'fs'
import path from 'path'
import { importIndexedDir } from '../src/importIndexedDir'

test('importIndexedDir() keepModulesDir merges node_modules', async () => {
  const tmp = tempDir()
  fs.mkdirSync(path.join(tmp, 'src/node_modules/a'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'src/node_modules/a/index.js'), 'module.exports = 1')

  fs.mkdirSync(path.join(tmp, 'dest/node_modules/b'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'dest/node_modules/b/index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  const filenames = {
    'node_modules/a/index.js': path.join(tmp, 'src/node_modules/a/index.js'),
  }
  importIndexedDir(fs.linkSync, newDir, filenames, { keepModulesDir: true })

  expect(fs.readdirSync(path.join(newDir, 'node_modules'))).toEqual(['a', 'b'])
})
