import JSON5 from 'json5'
import stripBom from 'strip-bom'
import parseJson from 'parse-json'

import gfs from '@pnpm/graceful-fs'
import type { ProjectManifest } from '@pnpm/types'

export async function readJson5File(filePath: string): Promise<{
  data: ProjectManifest;
  text: string;
}> {
  const text = await readFileWithoutBom(filePath)

  try {
    return {
      data: JSON5.parse(text),
      text,
    }
  } catch (err: unknown) {
    // @ts-ignore
    err.message = `${err.message as string} in ${filePath}`
    // @ts-ignore
    err.code = 'ERR_PNPM_JSON5_PARSE'
    throw err
  }
}

export async function readJsonFile(filePath: string): Promise<{
  data: ProjectManifest;
  text: string;
}> {
  const text = await readFileWithoutBom(filePath)

  try {
    return {
      data: parseJson(text, filePath) as ProjectManifest,
      text,
    }
  } catch (err: unknown) {
    // @ts-ignore
    err.code = 'ERR_PNPM_JSON_PARSE'
    throw err
  }
}

async function readFileWithoutBom(path: string): Promise<string> {
  return stripBom(await gfs.readFile(path, 'utf8'))
}
