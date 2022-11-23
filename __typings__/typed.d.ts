// This file contains type definitions that aren't just `export = any`

declare module 'cli-columns' {
  function cliColumns (values: string[], opts?: { newline?: string, width?: number }): string
  export = cliColumns;
}

declare module 'normalize-registry-url' {
  function normalizeRegistryUrl (registry: string): string
  export = normalizeRegistryUrl;
}

declare module 'normalize-newline' {
  function normalizeNewline (text: string): string
  export = normalizeNewline;
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

declare module 'strip-comments-strings' {
  export interface CodeItem {
    // What feature of the code has been found:
    type: string,
    // The indices of the feature in the original code string:
    index: number,
    indexEnd: number,
    // The test of the feature
    content: string
  }
  export interface CodeAttributes {
    // The remaining code text after all features have been stripped:
    text: string,
    // The items found:
    comments: CodeItem[],
    regexes: CodeItem[],
    strings: CodeItem[]
  }
  export function parseString (str: string): CodeAttributes;
  export type CodeItemReplacer = (item: CodeItem) => string;
  export function stripComments (
    str: string, replacer?: CodeItemReplacer): string;
}

declare module 'bin-links/lib/fix-bin' {
  function fixBin (path: string, execMode: number): Promise<void>;
  export = fixBin;
}

declare namespace NodeJS.Module {
  function _nodeModulePaths(from: string): string[]
}
