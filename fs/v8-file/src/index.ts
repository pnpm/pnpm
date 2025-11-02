import fs from 'fs'
import v8 from 'v8'
import util from 'util'

export function safeReadV8FileSync <T> (filePath: string): T | undefined {
  try {
    return readV8FileSync<T>(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') return undefined
    throw error
  }
}

export function readV8FileSync <T> (filePath: string): T | undefined {
  const buffer: Buffer = fs.readFileSync(filePath)
  try {
    return v8.deserialize(buffer)
  } catch {
    return undefined
  }
}

export function readV8FileStrictSync <T> (filePath: string): T {
  const buffer: Buffer = fs.readFileSync(filePath)
  return v8.deserialize(buffer)
}

export async function readV8FileStrictAsync <T> (filePath: string): Promise<T> {
  const buffer: Buffer = await fs.promises.readFile(filePath)
  return v8.deserialize(buffer)
}
