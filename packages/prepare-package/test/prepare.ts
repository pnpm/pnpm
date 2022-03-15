import { promises as fs } from 'fs'
import { runPrepareHook, filterFilesIndex } from '@pnpm/prepare-package'
import createCafs from '@pnpm/cafs'
import fixtures from '@pnpm/test-fixtures'

const f = fixtures(__dirname)

test('preparePackage runs a prepare script if one is present', async () => {
  const pkgDir = f.prepare('with-prepare-script')

  try {
    await fs.unlink(`${pkgDir}/file`)
  } catch (e) {}

  await runPrepareHook(pkgDir)

  expect(await fs.stat(`${pkgDir}/file`)).toBeTruthy()
})

test('preparePackage removes files which should not be part of a package', async () => {
  const pkgDir = f.prepare('with-spurious-files')

  try {
    await fs.unlink(`${pkgDir}/spurious.js`)
  } catch (e) {}

  await fs.copyFile(`${pkgDir}/file.js`, `${pkgDir}/spurious.js`)

  const index = createCafs(pkgDir)

  const filtered = await filterFilesIndex(pkgDir, index)

  expect(index[`${pkgDir}/spurious.js`]).toBeTruthy()
  expect(filtered[`${pkgDir}/spurious.js`]).toBeFalsy()
})
