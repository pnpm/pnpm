import test = require('tape')
import {read, write, Modules} from '@pnpm/modules-yaml'
import tempy = require('tempy')

test('write() and read()', async (t) => {
  const modulesYaml = {} as Modules
  const tempDir = tempy.directory()
  await write(tempDir, modulesYaml)
  t.deepEqual(await read(tempDir), modulesYaml)
  t.end()
})
