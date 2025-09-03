import fs from 'fs'
import util from 'util'

export async function safeStat (filePath: string): Promise<fs.Stats | undefined> {
  try {
    return await fs.promises.stat(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
}

export function safeStatSync (filePath: string): fs.Stats | undefined {
  try {
    return fs.statSync(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
}
