import { rm } from 'node:fs/promises'

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
 * there, so anything reaching the caller is unexpected, and downgrading
 * it to a warning would let the leak this teardown prevents come back
 * unnoticed. `maxRetries` covers the transient case where the
 * just-killed server has not released its handles yet, which Windows is
 * prone to.
 */
async function removeStorage () {
  const storage = global.registryMockStorage
  if (storage == null) return
  await rm(storage, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
}
