import fs from 'fs'
import v8 from 'v8'
import util from 'util'

export function safeReadV8FileSync <T> (filePath: string): T | undefined {
  let buffer!: Buffer
  try {
    buffer = fs.readFileSync(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') return undefined
    throw error
  }
  try {
    return v8.deserialize(buffer)
  } catch {
    return undefined
  }
}
