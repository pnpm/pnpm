import fs = require('fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import createCafs from '../src'

test('unpack', async (t) => {
  const dest = tempy.directory()
  t.comment(dest)
  const cafs = createCafs(dest)
  await cafs.addFilesFromTarball(
    fs.createReadStream(path.join(__dirname, '../__fixtures__/babel-helper-hoist-variables-6.24.1.tgz')),
  )
  t.end()
})
