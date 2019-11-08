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

// TODO: Has @types declaration
declare module 'is-ci' {
  const isCI: boolean;
  export = isCI;
}

// TODO: Has @types declaration
declare module 'is-windows' {
  function isWindows(): boolean;
  export = isWindows;
}

declare module 'normalize-registry-url' {
  function normalizeRegistryUrl (registry: string): string
  export = normalizeRegistryUrl;
}

// TODO: Has @types declaration
declare module 'pretty-time' {
  function prettyTime (time: [number, number]): string;
  export = prettyTime;
}

declare module 'read-ini-file' {
  function readIniFile (filename: string): Promise<Object>;
  export = readIniFile;
}

declare module 'tape-promise' {
  import tape = require('tape')
  export = tapePromise;

  function tapePromise(tape: any): (name: string, cb: tape.TestCase) => void;
  function tapePromise(tape: any): (name: string, opts: tape.TestOptions, cb: tape.TestCase) => void;
  function tapePromise(tape: any): (cb: tape.TestCase) => void;
  function tapePromise(tape: any): (opts: tape.TestOptions, cb: tape.TestCase) => void;
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
