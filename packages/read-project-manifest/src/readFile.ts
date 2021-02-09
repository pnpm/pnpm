import { ProjectManifest } from '@pnpm/types'
import { promises as fs } from 'fs'
import JSON5 = require('json5')
import parseJson = require('parse-json')
import stripBom = require('strip-bom')

export async function readJson5File (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  try {
    return {
      data: JSON5.parse(text) as ProjectManifest,
      text,
    }
  } catch (err) {
    err.message = `${err.message as string} in ${filePath}`
    err['code'] = 'ERR_PNPM_JSON5_PARSE'
    throw err
  }
}

export async function readJsonFile (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  try {
    return {
      data: parseJson(text, filePath) as ProjectManifest,
      text,
    }
  } catch (err) {
    err['code'] = 'ERR_PNPM_JSON_PARSE'
    throw err
  }
}

async function readFileWithoutBom (path: string) {
  return stripBom(await fs.readFile(path, 'utf8'))
}
