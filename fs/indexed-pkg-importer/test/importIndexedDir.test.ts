import { tempDir } from '@pnpm/prepare'
import { promises as fs, mkdirSync, writeFileSync } from 'fs'
import path from 'path'
import { importIndexedDir } from '../src/importIndexedDir'

test('importIndexedDir() keepModulesDir merges node_modules', async () => {
  const tmp = tempDir()
  mkdirSync(path.join(tmp, 'src/node_modules/a'), { recursive: true })
  writeFileSync(path.join(tmp, 'src/node_modules/a/index.js'), 'module.exports = 1')

  mkdirSync(path.join(tmp, 'dest/node_modules/b'), { recursive: true })
  writeFileSync(path.join(tmp, 'dest/node_modules/b/index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  const filenames = {
    'node_modules/a/index.js': path.join(tmp, 'src/node_modules/a/index.js'),
  }
  await importIndexedDir(fs.link, newDir, filenames, { keepModulesDir: true })

  expect(await fs.readdir(path.join(newDir, 'node_modules'))).toEqual(['a', 'b'])
})
