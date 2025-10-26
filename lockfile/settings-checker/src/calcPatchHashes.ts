import path from 'path'
import pMapValues from 'p-map-values'
import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { type PatchFile } from '@pnpm/lockfile.types'

export async function calcPatchHashes (patches: Record<string, string>, lockfileDir: string): Promise<Record<string, PatchFile>> {
  return pMapValues(async (patchFilePath) => {
    return {
      hash: await createHexHashFromFile(patchFilePath),
      path: path.relative(lockfileDir, patchFilePath).replaceAll('\\', '/'),
    }
  }, patches)
}
