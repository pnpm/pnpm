import fs from 'fs'
import isWindows from 'is-windows'
import isExe from 'isexe'

const IS_WINDOWS = isWindows()

// eslint-disable-next-line
export default (ok: (value: any, comment: string) => void, filePath: string): void => {
  if (IS_WINDOWS) {
    if (fs.existsSync(`${filePath}.cmd`)) {
      ok(isExe.sync(`${filePath}.cmd`), `${filePath}.cmd is executable`)
    } else {
      ok(isExe.sync(`${filePath}.exe`), `${filePath}.exe is executable`)
    }
    return
  }

  const stat = fs.statSync(filePath)
  ok((stat.mode & 0o111) === 0o111, `${filePath} is executable`)
  ok(stat.isFile(), `${filePath} refers to a file`)
}
