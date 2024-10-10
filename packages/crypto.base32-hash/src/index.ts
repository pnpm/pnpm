import crypto from 'crypto'
import fs from 'fs'
import { base32 } from 'rfc4648'

const hash =
  // @ts-expect-error -- crypto.hash is supported in Node 21.7.0+, 20.12.0+
  crypto.hash ??
  ((
    algorithm: string,
    data: crypto.BinaryLike,
    outputEncoding: crypto.BinaryToTextEncoding
  ) => crypto.createHash(algorithm).update(data).digest(outputEncoding))

export function createBase32Hash (str: string): string {
  return base32.stringify(hash('md5', str)).replace(/(=+)$/, '').toLowerCase()
}

export async function createBase32HashFromFile (file: string): Promise<string> {
  const content = await fs.promises.readFile(file, 'utf8')
  return createBase32Hash(content.split('\r\n').join('\n'))
}
