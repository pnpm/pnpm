import path from 'path'
import { preparePackage } from '@pnpm/prepare-package'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as loadJsonFile } from 'load-json-file'

const f = fixtures(__dirname)

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  f.copy('has-prepublish-script', tmp)
  await preparePackage({ rawConfig: {} }, tmp)
  expect(loadJsonFile(path.join(tmp, 'output.json'))).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package does not run the prebublish script if the main file is present', async () => {
  const tmp = tempDir()
  f.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ rawConfig: {} }, tmp)
  expect(loadJsonFile(path.join(tmp, 'output.json'))).toStrictEqual([
    'prepublish',
  ])
})
