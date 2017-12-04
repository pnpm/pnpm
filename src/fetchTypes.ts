import * as unpackStream from 'unpack-stream'
import {Resolution} from './resolveTypes'

export interface FetchOptions {
  cachedTarballLocation: string,
  pkgId: string,
  prefix: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
}

export type FetchFunction = (
  resolution: Resolution,
  target: string,
  opts: FetchOptions,
) => Promise<unpackStream.Index>
