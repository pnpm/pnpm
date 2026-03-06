import gfs from '@pnpm/graceful-fs'

/**
 * Write data to a file in JSON format (synchronous)
 */
export function writeMsgpackFileSync (filePath: string, data: unknown): void {
  const json = JSON.stringify(data)
  gfs.writeFileSync(filePath, json, 'utf8')
}

/**
 * Read JSON data from a file (synchronous)
 */
export function readMsgpackFileSync<T> (filePath: string): T {
  const content = gfs.readFileSync(filePath, 'utf8')
  return JSON.parse(content) as T
}

/**
 * Read JSON data from a file (async)
 */
export async function readMsgpackFile<T> (filePath: string): Promise<T> {
  const content = await gfs.readFile(filePath, 'utf8')
  return JSON.parse(content) as T
}

/**
 * Write data to a file in JSON format (async)
 */
export async function writeMsgpackFile (filePath: string, data: unknown): Promise<void> {
  const json = JSON.stringify(data)
  await gfs.writeFile(filePath, json, 'utf8')
}
