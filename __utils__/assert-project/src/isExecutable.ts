import fs from 'fs'
import isWindows from 'is-windows'
import { sync as isexe } from 'isexe'

const IS_WINDOWS = isWindows()

// eslint-disable-next-line
export default (ok: (value: any, comment: string) => void, filePath: string) => {
  if (IS_WINDOWS) {
    ok(isexe(`${filePath}.cmd`), `${filePath}.cmd is executable`)
    return
  }

  const stat = fs.statSync(filePath)
  ok((stat.mode & 0o111) === 0o111, `${filePath} is executable`)
  ok(stat.isFile(), `${filePath} refers to a file`)
}
