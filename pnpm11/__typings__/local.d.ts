/* eslint-disable @typescript-eslint/no-explicit-any -- Untyped third-party modules expose opaque values. */
declare module 'bole' {
  const anything: any
  export = anything
}

declare module 'split2' {
  const anything: any
  export = anything
}

declare module 'hyperdrive-schemas' {
  const anything: any
  export = anything
}

declare module 'fuse-native' {
  const anything: any
  export = anything
}

declare module '@zkochan/libnpx/index' {
  const anything: any
  export = anything
}

declare module '@pnpm/byline' {
  import type { Readable } from 'node:stream'
  function byline (stream: Readable): Readable
  export = byline
}

declare module 'uid-number' {
  function uidNumber (
    user: string | number | null | undefined,
    group: string | number | null | undefined,
    cb: (err: Error | null, uid: number, gid: number) => void
  ): void
  export = uidNumber
}

declare module '@pnpm/npm-package-arg' {
  const anything: any
  export = anything
}

declare module '@pnpm/which' {
  const anything: any
  export = anything
}

declare module 'anonymous-npm-registry-client' {
  const anything: any
  export = anything
}

declare module 'ansi-diff' {
  const anything: any
  export = anything
}

declare module 'better-path-resolve' {
  export const betterPathResolve: (...args: any[]) => any
}

// cspell:disable-next-line
declare module '@zkochan/diable' {
  const anything: any
  export = anything
}

declare module 'dint' {
  const anything: any
  export = anything
}

declare module 'exists-link' {
  const anything: any
  export = anything
}

declare module 'is-inner-link' {
  export function isInnerLink (parent: string, relativePathToLink: string): Promise<{ isInner: boolean, target: string }>
}

declare module 'read-package-json' {
  const anything: any
  export = anything
}

declare module 'split-cmd/index.modern.mjs' {
  export function split (command: string): string[]
}

declare module 'stacktracey' {
  const anything: any
  export = anything
}

declare module 'yaml-tag' {
  const anything: any
  export = anything
}

declare module '@pnpm/patch-package/dist/applyPatches.js' {
  export function applyPatch (opts: any): boolean
}

declare module '@pnpm/patch-package/dist/patch/parse.js' {
  export interface PatchFilePart {
    type: 'file deletion' | 'file creation' | 'patch' | 'mode change' | 'rename'
    path?: string
    fromPath?: string
    toPath?: string
  }
  export function parsePatchFile (file: string): PatchFilePart[]
}

declare module 'ramda/src/map' {
  function map <Key extends string | number | symbol, SourceValue, MappedValue> (fn: (value: SourceValue) => MappedValue, obj: Record<Key, SourceValue>): Record<Key, MappedValue>
  export = map
}

declare module '@yarnpkg/core/semverUtils'
declare module '@yarnpkg/core/structUtils'

// These are needed for transitive type dependencies that can't be resolved
// from within the pnpm store
declare module 'picomatch' {
  const anything: any
  export = anything
}

declare module 'seek-bzip' {
  interface Input {
    readByte: () => number
    read: (buffer: Uint8Array, offset: number, length: number) => number
  }
  interface Output {
    writeByte: (byte: number) => void
  }
  const Bunzip: {
    decode: {
      (input: Uint8Array | Input): Buffer
      (input: Uint8Array | Input, output: Output): void
    }
  }
  export = Bunzip
}
