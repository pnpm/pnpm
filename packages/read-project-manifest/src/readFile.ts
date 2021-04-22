import { promisify } from 'util'
import gracefulFs from 'graceful-fs'
import { ProjectManifest } from '@pnpm/types'
import JSON5 from 'json5'
import parseJson from 'parse-json'
import stripBom from 'strip-bom'

// We use graceful-fs to avoid EMFILE errors.
const readFile = promisify(gracefulFs.readFile)

export async function readJson5File (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  try {
    return {
      data: JSON5.parse(text),
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
  return stripBom(await readFile(path, 'utf8'))
}
