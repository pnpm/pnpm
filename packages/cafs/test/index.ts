import createCafs from '../src'
import fs = require('fs')
import path = require('path')
import tempy = require('tempy')

describe('cafs', () => {
  test('unpack', async () => {
    const dest = tempy.directory()
    const cafs = createCafs(dest)
    const filesIndex = await cafs.addFilesFromTarball(
      fs.createReadStream(path.join(__dirname, '../__fixtures__/node-gyp-6.1.0.tgz'))
    )
    expect(Object.keys(filesIndex)).toHaveLength(121)
  })
})
