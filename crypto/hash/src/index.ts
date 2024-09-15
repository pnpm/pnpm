import crypto from 'crypto'
import fs from 'fs'

export function createShortHash (input: string): string {
  const hash = crypto.createHash('sha256')
  hash.update(input)
  return hash.digest('hex').substring(0, 32)
}

export function createHash (input: string): string {
  const hash = crypto.createHash('sha256')
  hash.update(input)
  return `sha256-${hash.digest('base64')}`
}

export async function createHashFromFile (file: string): Promise<string> {
  const content = await fs.promises.readFile(file, 'utf8')
  return createHash(content.split('\r\n').join('\n'))
}
