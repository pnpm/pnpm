import fs from 'node:fs'

export interface Options {
  /**
   * If a PowerShell script should be created.
   *
   * @default true
   */
  createPwshFile?: boolean

  /**
   * If a Windows Command Prompt script should be created.
   *
   * @default true on Windows, false on other platforms
   */
  createCmdFile?: boolean

  /**
   * If symbolic links should be preserved.
   *
   * @default false
   */
  preserveSymlinks?: boolean

  /**
   * The path to the executable file.
   */
  prog?: string

  /**
   * The arguments to initialize the `node` process with.
   */
  args?: string

  /**
   * The arguments to initialize the target process with, before the actual CLI arguments
   */
  progArgs?: string[]

  /**
   * The value of the $NODE_PATH environment variable.
   *
   * The single `string` format is only kept for legacy compatibility,
   * and the array form should be preferred.
   */
  nodePath?: string | string[]

  /**
   * fs implementation to use.  Must implement node's `fs` module interface.
   */
  fs?: typeof import('fs')

  /*
   * Path to the Node.js executable
   */
  nodeExecPath?: string

  prependToPath?: string

  /**
   * The directory the shell shim computes its relative target from, as
   * {@link getShShimDir} returns it. Computed when omitted.
   */
  shShimDir?: string
}

export interface GetShShimDirOptions {
  /** The shim directory with its symlinks resolved, when the caller already has it. */
  physicalDir?: string
  fs?: ShimDirFs
}

export type ShimDirFs = Pick<typeof fs.promises, 'lstat' | 'realpath'>

export const DEFAULT_OPTIONS = {
  createPwshFile: true,
  createCmdFile: process.platform === 'win32',
}

/**
 * @internal
 */
export type InternalOptions = Options & Required<Pick<Options, keyof typeof DEFAULT_OPTIONS>> & {
  fs_: FsPromises
  isTargetMissing?: boolean
  /** Reject a missing source instead of shimming it. */
  requireSource?: boolean
}

export type FsPromises = Pick<typeof fs.promises, 'chmod' | 'lstat' | 'mkdir' | 'readFile' | 'realpath' | 'stat' | 'unlink' | 'writeFile'>

export type ShimGenerator = (src: string, to: string, opts: InternalOptions) => string

export interface ShimGenExtTuple {
  generator: ShimGenerator
  extension: string
}

export interface RuntimeInfo {
  program: string | null
  additionalArgs: string
  /** Whether `program` was inferred from the path because the target is missing. */
  isTargetMissing?: boolean
}
