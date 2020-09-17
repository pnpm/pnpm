import createCafs from '../src'
import fs = require('fs')
import path = require('path')
import tempy = require('tempy')

describe('unpack', () => {
  test('unpack', async () => {
    const dest = tempy.directory()
    // t.comment(dest)
    console.log(test)
    const cafs = createCafs(dest)
    await cafs.addFilesFromTarball(
      fs.createReadStream(path.join(__dirname, '../__fixtures__/node-gyp-6.1.0.tgz'))
    )
    //  t.end()
  })
})
