import fs from 'node:fs'

import { isError } from '@pnpm/error'
import type { FilesMap } from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'

import { parseJsonBufferSync } from './parseJson.js'

export function readManifestFromCafs (filesMap: FilesMap): DependencyManifest | undefined {
  const manifestPath = filesMap.get('package.json')
  if (manifestPath == null) return undefined
  try {
    return parseJsonBufferSync(fs.readFileSync(manifestPath)) as DependencyManifest
  } catch (err: unknown) {
    if (err instanceof SyntaxError || (isError(err) && 'code' in err && err.code === 'ENOENT')) {
      return undefined
    }
    throw err
  }
}
