import fs = require('mz/fs')
import path = require('path')

export default async function realNodeModulesDir (prefix: string): Promise<string> {
  const dirName = path.join(prefix, 'node_modules')
  try {
    return await fs.realpath(dirName)
  } catch (err) {
    if (err['code'] === 'ENOENT') {
      return dirName
    }
    throw err
  }
}
