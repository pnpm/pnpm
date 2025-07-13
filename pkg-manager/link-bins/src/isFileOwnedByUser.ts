import { promises as fs } from 'fs'

export async function isFileOwnedByUser (filePath: string): Promise<boolean> {
  try {
    const stats = await fs.stat(filePath)
    const uid = process.getuid ? process.getuid() : process.pid // Fallback for environments without getuid
    return stats.uid === uid
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return false // File does not exist
    }
    throw error // Re-throw other errors
  }
}
