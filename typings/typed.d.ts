// This file contains type definitions that aren't just `export = any`

declare module '@pnpm/registry-mock' {
  export function getIntegrity (pkgName: string, pkgVersion: string): string
  export function addDistTag (opts: {package: string, version: string, distTag: string}): Promise<void>
  export const REGISTRY_MOCK_PORT: string
}

declare module 'cli-columns' {
  function cliColumns (values: string[], opts?: { newline?: string, width?: number }): string
  export = cliColumns;
}

declare module 'normalize-registry-url' {
  function normalizeRegistryUrl (registry: string): string
  export = normalizeRegistryUrl;
}

declare module 'path-name' {
  const pathname: string;
  export = pathname;
}

declare module 'read-ini-file' {
  function readIniFile (filename: string): Promise<Object>;
  export = readIniFile;
}

declare module 'right-pad' {
  function rightPad (txt: string, size: number): string;
  export = rightPad;
}

declare module 'semver-utils' {
  export function parseRange (range: string): Array<{
    semver?: string,
    operator: string,
    major?: string,
    minor?: string,
    patch?: string,
  }>
}

declare module 'split-cmd' {
  export function split (cmd: string): string[]
  export function splitToObject (cmd: string): { command: string, args: string[] }
}

declare namespace NodeJS.Module {
  function _nodeModulePaths(from: string): string[]
}
