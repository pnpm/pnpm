import fs from 'fs'
import path from 'path'
import { install } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from '../utils'

test('installing with node-modules node-linker', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      send: '0.17.2',
      'has-flag': '1.0.0',
      ms: '1.0.0',
    },
  }, await testDefaults({
    nodeLinker: 'node-modules',
  }))

  expect(fs.realpathSync('node_modules/send')).toEqual(path.resolve('node_modules/send'))
  expect(fs.realpathSync('node_modules/has-flag')).toEqual(path.resolve('node_modules/has-flag'))
  expect(fs.realpathSync('node_modules/ms')).toEqual(path.resolve('node_modules/ms'))
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()
})
