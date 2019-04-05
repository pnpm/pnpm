import isWindows = require('is-windows')
import isexeCB = require('isexe')
import fs = require('mz/fs')
import {Test} from 'tape'
import { promisify } from 'util'

const IS_WINDOWS = isWindows()
const isexe = promisify(isexeCB)

export default async (t: Test, filePath: string) => {
  if (IS_WINDOWS) {
    t.ok(await isexe(`${filePath}.cmd`), `${filePath}.cmd is executable`)
    return
  }

  const stat = await fs.stat(filePath)
  t.equal(stat.mode, parseInt('100755', 8), `${filePath} is executable`)
  t.ok(stat.isFile(), `${filePath} refers to a file`)
}
