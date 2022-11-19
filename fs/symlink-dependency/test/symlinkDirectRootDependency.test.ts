import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'

test('symlink is created to directory that does not yet exist', async () => {
  const tmp = tempDir(false)
  const destModulesDir = path.join(tmp, 'node_modules')
  const dependencyLocation = path.join(tmp, 'dep')
  fs.mkdirSync(destModulesDir)
  await symlinkDirectRootDependency(dependencyLocation, destModulesDir, 'dep', {
    linkedPackage: {
      name: 'dep',
      version: '1.0.0',
    },
    prefix: '',
  })
  fs.mkdirSync(dependencyLocation)
  fs.writeFileSync(path.join(dependencyLocation, 'index.js'), 'module.exports = {}')
  expect(fs.existsSync(path.join(destModulesDir, 'dep/index.js'))).toBe(true)
})
