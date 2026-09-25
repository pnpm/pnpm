import { createHexHashFromFile } from '@pnpm/crypto.hash'

export async function calcPatchHashes (patches: Record<string, string>): Promise<Record<string, string>> {
  const hashes = await Promise.all(
    Object.entries(patches).map(async ([patchKey, patchFilePath]): Promise<[string, string]> =>
      [patchKey, await createHexHashFromFile(patchFilePath)]
    )
  )
  return Object.fromEntries(hashes)
}
