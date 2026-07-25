import { rm } from 'node:fs/promises'

export default async () => {
  // Kill the server before removing its storage: pnpr writes proxy-cache
  // entries as it serves, so removing the directory underneath a live
  // server races with those writes.
  await global.killServer?.()

  const storage = global.registryMockStorage
  if (storage == null) return
  try {
    await rm(storage, { recursive: true, force: true })
  } catch (err) {
    // A leaked temp directory is not worth failing an otherwise-green
    // run over, but it should not be silent either — it is the thing
    // that eventually fills /tmp.
    console.warn(`Failed to remove the registry mock storage at ${storage}:`, err)
  }
}
