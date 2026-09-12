import { rm } from 'node:fs/promises'
import path from 'node:path'

import { STORAGE_PREFIX } from './storagePrefix.js'

export default async () => {
  // The server is killed before its storage is removed: pnpr writes
  // proxy-cache entries as it serves, so removing the directory
  // underneath a live server races with those writes.
  //
  // Cleanup still runs when the shutdown fails, though. A `killServer`
  // that rejects or times out would otherwise skip it and leave behind
  // the directory this teardown exists to remove.
  let shutdownError
  try {
    await global.killServer?.()
  } catch (error) {
    shutdownError = error
  }

  try {
    await removeStorage()
  } catch (error) {
    // A failed shutdown is the likeliest cause of a failed removal — a
    // server still holding the directory — so report both rather than
    // letting the second failure hide the first.
    throw shutdownError == null
      ? error
      : new AggregateError([shutdownError, error], 'registry mock teardown failed')
  }

  if (shutdownError != null) throw shutdownError
}

/**
 * Remove the storage `globalSetup` created for this run.
 *
 * Errors propagate: `force` already ignores a directory that isn't
 * there, so anything reaching the caller is unexpected, and a warning
 * would be easy to miss in a passing run — which is how storage
 * accumulates in `/tmp` unnoticed. `maxRetries` covers the transient
 * case where the just-killed server has not released its handles yet,
 * which Windows is prone to.
 */
async function removeStorage () {
  const storage = global.registryMockStorage
  if (storage == null) return
  // This is a recursive force-delete, so it only ever runs against a
  // path shaped like the one `globalSetup` mkdtemp'd. Anything else
  // means the global was set by something other than that setup, and
  // guessing at its intent is not worth the blast radius.
  if (!path.basename(storage).startsWith(STORAGE_PREFIX)) {
    throw new Error(
      `Refusing to remove ${storage}: not a ${STORAGE_PREFIX}* directory created by globalSetup.`
    )
  }
  await rm(storage, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
}
