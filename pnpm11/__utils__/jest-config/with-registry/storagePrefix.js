/**
 * Basename prefix of the per-run registry storage directory.
 *
 * Shared so `globalTeardown` can check that what it is about to
 * recursively delete is a directory `globalSetup` created.
 */
export const STORAGE_PREFIX = 'pnpm-registry-mock-storage-'
