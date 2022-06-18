import crypto from 'crypto'
import fs from 'fs'
import { base32 } from 'rfc4648'

export function createBase32Hash (str: string): string {
  return base32.stringify(crypto.createHash('md5').update(str).digest()).replace(/(=+)$/, '').toLowerCase()
}

export async function createBase32HashFromFile (file: string): Promise<string> {
  return createBase32Hash(await fs.promises.readFile(file, 'utf8'))
}
