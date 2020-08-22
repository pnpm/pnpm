import createCafs from '../src'
import fs = require('fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

test('unpack', async (t) => {
  const dest = tempy.directory()
  t.comment(dest)
  const cafs = createCafs(dest)
  await cafs.addFilesFromTarball(
    fs.createReadStream(path.join(__dirname, '../__fixtures__/node-gyp-6.1.0.tgz'))
  )
  t.end()
})
