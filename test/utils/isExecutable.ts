import isWindows = require('is-windows')
import isexe = require('isexe')
import fs = require('mz/fs')
import {Test} from 'tape'

const IS_WINDOWS = isWindows()

export default async function isExecutable (t: Test, filePath: string) {
  if (IS_WINDOWS) {
    t.ok(isexe(filePath), `${filePath} is executable`)
    return
  }

  const stat = await fs.stat(filePath)
  t.equal(stat.mode, parseInt('100755', 8), `${filePath} is executable`)
  t.ok(stat.isFile(), `${filePath} refers to a file`)
  return
}
