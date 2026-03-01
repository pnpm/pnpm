export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const LOCKFILE_MAJOR_VERSION = '9'
export const LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.0`

export const MANIFEST_BASE_NAMES = ['package.json', 'package.json5', 'package.yaml'] as const

export const ENGINE_NAME = `${process.platform};${process.arch};node${process.version.split('.')[0].substring(1)}`
export const LAYOUT_VERSION = 5
export const STORE_VERSION = 'v11'
export const GLOBAL_LAYOUT_VERSION = 'v11'

export const GLOBAL_CONFIG_YAML_FILENAME = 'config.yaml'
export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
export const ABBREVIATED_META_DIR = 'metadata-v1.3'
export const FULL_META_DIR = 'metadata-full-v1.3' // This is currently not used at all
export const FULL_FILTERED_META_DIR = 'metadata-ff-v1.3'

export function getNodeBinLocationForCurrentOS (platform: string = process.platform): string {
  return platform === 'win32' ? 'node.exe' : 'bin/node'
}

export function getNodeBinsForCurrentOS (platform: string = process.platform): Record<string, string> {
  if (platform === 'win32') {
    return {
      node: 'node.exe',
      npm: 'node_modules/npm/bin/npm-cli.js',
      npx: 'node_modules/npm/bin/npx-cli.js',
    }
  }
  return {
    node: 'bin/node',
    npm: 'lib/node_modules/npm/bin/npm-cli.js',
    npx: 'lib/node_modules/npm/bin/npx-cli.js',
  }
}

export function getDenoBinLocationForCurrentOS (platform: string = process.platform): string {
  return platform === 'win32' ? 'deno.exe' : 'deno'
}

export function getBunBinLocationForCurrentOS (platform: string = process.platform): string {
  return platform === 'win32' ? 'bun.exe' : 'bun'
}
