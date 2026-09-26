import fs from 'node:fs'
import util from 'node:util'

export function readCAFileSync (filePath: string): string[] | undefined {
  try {
    let contents = fs.readFileSync(filePath, 'utf8')
    // Normalize line endings to Unix-style
    contents = contents.replace(/\r\n/g, '\n');
    const delim = '-----END CERTIFICATE-----'
    const output = contents
      .split(delim)
      .filter((ca) => Boolean(ca.trim()))
      .map((ca) => `${ca.trimStart()}${delim}`)
    return output
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
}
