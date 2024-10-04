import path from 'path'
import pMapValues from 'p-map-values'
import { createBase32HashFromFile } from '@pnpm/crypto.base32-hash'
import { type PatchFile } from '@pnpm/lockfile.types'

export async function calcPatchHashes (patches: Record<string, string>, lockfileDir: string): Promise<Record<string, PatchFile>> {
  return pMapValues(async (patchFilePath) => {
    return {
      hash: await createBase32HashFromFile(patchFilePath),
      path: path.relative(lockfileDir, patchFilePath).replaceAll('\\', '/'),
    }
  }, patches)
}
