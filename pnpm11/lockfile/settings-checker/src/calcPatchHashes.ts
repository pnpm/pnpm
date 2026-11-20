
import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { isError, PnpmError } from '@pnpm/error'

export async function calcPatchHashes (patches: Record<string, string>): Promise<Record<string, string>> {
  const hashes = await Promise.all(
    Object.entries(patches).map(async ([patchKey, patchFilePath]): Promise<[string, string]> =>
      [patchKey, await calcPatchHash(patchFilePath)]
    )
  )
  return Object.fromEntries(hashes)
}

async function calcPatchHash (patchFilePath: string): Promise<string> {
  try {
    return await createHexHashFromFile(patchFilePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new PnpmError('PATCH_NOT_FOUND', `Patch file not found: ${patchFilePath}`)
    }
    throw err
  }
}
