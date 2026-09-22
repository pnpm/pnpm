import crypto from 'node:crypto'
import fs from 'node:fs'

import ssri from 'ssri'

export function createShortHash (input: string): string {
  return createHexHash(input).substring(0, 32)
}

export function createHexHash (input: string): string {
  return crypto.hash('sha256', input, 'hex')
}

export function createHash (input: string): string {
  return `sha256-${crypto.hash('sha256', input, 'base64')}`
}

export async function createHashFromMultipleFiles (files: string[]): Promise<string> {
  if (files.length === 1) {
    return createHashFromFile(files[0])
  }
  const hashes = await Promise.all(files.map(createHashFromFile))
  return createHash(hashes.join(','))
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

export async function getTarballIntegrity (filename: string): Promise<string> {
  const handle = await fs.promises.open(
    filename,
    fs.constants.O_RDONLY | (fs.constants.O_NONBLOCK ?? 0)
  )
  try {
    const stats = await handle.stat()
    if (!stats.isFile()) {
      throw new Error('expected a file')
    }
    return (await ssri.fromStream(handle.createReadStream({ autoClose: false }))).toString()
  } finally {
    await handle.close()
  }
}
