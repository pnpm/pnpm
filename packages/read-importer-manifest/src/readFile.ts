import { ImporterManifest } from '@pnpm/types'
import fs = require('graceful-fs')
import JSON5 = require('json5')
import stripBom = require('strip-bom')
import { promisify } from 'util'
const readFile = promisify(fs.readFile)

export async function readJson5File (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  return {
    data: JSON5.parse(text) as ImporterManifest,
    text,
  }
}

export async function readJsonFile (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  return {
    data: JSON.parse(text) as ImporterManifest,
    text,
  }
}

async function readFileWithoutBom (path: string) {
  return stripBom(await readFile(path, 'utf8'))
}
