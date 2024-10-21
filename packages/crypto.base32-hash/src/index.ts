import * as crypto from '@pnpm/crypto.polyfill'
import fs from 'fs'
import { base32 } from 'rfc4648'

export function createBase32Hash (str: string): string {
  return base32.stringify(crypto.hash('md5', str, 'buffer')).replace(/(=+)$/, '').toLowerCase()
}

export async function createBase32HashFromFile (file: string): Promise<string> {
  const content = await fs.promises.readFile(file, 'utf8')
  return createBase32Hash(content.split('\r\n').join('\n'))
}
