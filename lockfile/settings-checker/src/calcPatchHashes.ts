import { pMapValues } from 'p-map-values'
import { createHexHashFromFile } from '@pnpm/crypto.hash'

export async function calcPatchHashes (patches: Record<string, string>): Promise<Record<string, string>> {
  return pMapValues(async (patchFilePath: string) => {
    return createHexHashFromFile(patchFilePath)
  }, patches)
}
