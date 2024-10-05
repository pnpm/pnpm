export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const LOCKFILE_MAJOR_VERSION = '9'
export const LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.0`
export const LOCKFILE_VERSION_V6 = '6.0'

export const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`
export const LAYOUT_VERSION = 5

export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
export const META_DIR = 'metadata'
export const FULL_META_DIR = 'metadata-full'
export const FULL_FILTERED_META_DIR = 'metadata-v1.2'
