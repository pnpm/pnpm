import { promises as fs } from 'fs'

export async function isFileOwnedByUser (filePath: string): Promise<boolean> {
  const { uid: fileOwnerId } = await fs.stat(filePath)
  const userId = process.getuid?.()
  return fileOwnerId === userId
}
