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

export interface TarballIntegrityOptions {
  algorithms?: string[]
}

export async function getTarballIntegrity (filename: string, opts?: TarballIntegrityOptions): Promise<string> {
  const handle = await fs.promises.open(
    filename,
    fs.constants.O_RDONLY | (fs.constants.O_NONBLOCK ?? 0)
  )
  try {
    const stats = await handle.stat()
    if (!stats.isFile()) {
      throw new Error('expected a file')
    }
    const algorithms = opts?.algorithms ?? ['sha512']
    return (await ssri.fromStream(handle.createReadStream({ autoClose: false }), { algorithms })).toString()
  } finally {
    await handle.close()
  }
}

export interface TarballIntegrityMatchResult {
  matches: boolean
  found: string
}

export function matchIntegrity (actual: string, expected: string): TarballIntegrityMatchResult {
  const actualParsed = ssri.parse(actual)
  const expectedParsed = ssri.parse(expected)
  if (!actualParsed || !expectedParsed) {
    return { matches: false, found: actual }
  }
  const match = actualParsed.match(expected)
  if (match) {
    return { matches: true, found: match.toString() }
  }
  const algo = Object.keys(expectedParsed)[0]
  const foundForAlgo = actualParsed[algo]?.[0]?.toString()
  return {
    matches: false,
    found: foundForAlgo ?? actualParsed.toString(),
  }
}
