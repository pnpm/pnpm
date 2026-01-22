import fs from 'fs'
import path from 'path'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import tempy from 'tempy'

test('skip directories.bin with real path traversal', async () => {
  // Create a secret file outside the package directory
  const tempDir = tempy.directory()
  const secretDir = path.join(tempDir, 'secret')
  fs.mkdirSync(secretDir)
  fs.writeFileSync(path.join(secretDir, 'secret.sh'), 'echo secret')

  // Create a package directory
  const pkgDir = path.join(tempDir, 'pkg')
  fs.mkdirSync(pkgDir)

  // Calculate relative path from pkgDir to secretDir
  const relativePath = path.relative(pkgDir, secretDir)

  // Attempt path traversal
  const bins = await getBinsFromPackageManifest({
    name: 'malicious',
    version: '1.0.0',
    directories: {
      bin: relativePath,
    },
  }, pkgDir)

  // Should be empty because it escaped pkgDir
  expect(bins).toStrictEqual([])
})
