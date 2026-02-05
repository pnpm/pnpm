import gfs from '@pnpm/graceful-fs'
import { type ProjectManifest } from '@pnpm/types'
import JSON5 from 'json5'
import parseJson from 'parse-json'
import stripBom from 'strip-bom'
import stripJsonComments from 'strip-json-comments'

export async function readJson5File (filePath: string): Promise<{ data: ProjectManifest, text: string }> {
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

export async function readJsoncFile (filePath: string): Promise<{ data: ProjectManifest, text: string }> {
  const text = await readFileWithoutBom(filePath)
  try {
    return {
      // JSONC is just JSON with comments and trailing commas, so use stripJsonComments to trim them and we can use JSON.parse immediately.
      // Although JSONC is also JSON5, we do not use JSON5.parse here because it may unexpectedly parse JSON5-specific data that is not JSONC-compatible.
      data: JSON.parse(stripJsonComments(text, { trailingCommas: true })),
      text,
    }
  } catch (err: any) { // eslint-disable-line
    err.message = `${err.message as string} in ${filePath}`
    err['code'] = 'ERR_PNPM_JSONC_PARSE'
    throw err
  }
}

export async function readJsonFile (filePath: string): Promise<{ data: ProjectManifest, text: string }> {
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

async function readFileWithoutBom (path: string): Promise<string> {
  return stripBom(await gfs.readFile(path, 'utf8'))
}
