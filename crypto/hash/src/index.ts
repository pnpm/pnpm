import crypto from 'crypto'
import fs from 'fs'

export function createShortHash (input: string): string {
  return createHexHash(input).substring(0, 32)
}

export function createHexHash (input: string): string {
  const hash = crypto.createHash('sha256')
  hash.update(input)
  return hash.digest('hex')
}

export function createHash (input: string): string {
  const hash = crypto.createHash('sha256')
  hash.update(input)
  return `sha256-${hash.digest('base64')}`
}

export async function createHashFromFile (file: string): Promise<string> {
  return createHash(await readNormalizedFile(file))
}

export async function createHexHashFromFile (file: string): Promise<string> {
  return createHexHash(await readNormalizedFile(file))
}

async function readNormalizedFile (file: string): Promise<string> {
  const content = await fs.promises.readFile(file, 'utf8')
  return content.split('\r\n').join('\n')
}
