import gfs from '@pnpm/graceful-fs'
import { ProjectManifest } from '@pnpm/types'
import JSON5 from 'json5'
import parseJson from 'parse-json'
import stripBom from 'strip-bom'

export async function readJson5File (filePath: string) {
  const text = await readFileWithoutBom(filePath)
  try {
    return {
      data: JSON5.parse(text),
      text,
    }
  } catch (err: any) { // eslint-disable-line
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
  } catch (err: any) { // eslint-disable-line
    err['code'] = 'ERR_PNPM_JSON_PARSE'
    throw err
  }
}

async function readFileWithoutBom (path: string) {
  return stripBom(await gfs.readFile(path, 'utf8'))
}
