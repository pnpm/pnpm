import { type InstallCommandOptions } from './install.js'

export type GetFetchFullMetadataOptions = Pick<InstallCommandOptions, 'supportedArchitectures' | 'rootProjectManifest'>

/**
 * This function is a workaround for the fact that npm registry's abbreviated metadata currently does not contain `libc`.
 *
 * See <https://github.com/pnpm/pnpm/issues/7362#issuecomment-1971964689>.
 */
export const getFetchFullMetadata = (opts: GetFetchFullMetadataOptions): true | undefined => (
  opts.supportedArchitectures?.libc ??
  opts.rootProjectManifest?.pnpm?.supportedArchitectures?.libc
) && true
