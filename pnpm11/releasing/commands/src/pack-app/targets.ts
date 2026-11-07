import { PnpmError } from '@pnpm/error'

// Target OS names match `process.platform`. That keeps the CLI surface
// consistent with pnpm's own `--os` flag (which also takes platform constants)
// and with `supportedArchitectures.os` in pnpm-workspace.yaml.
const SUPPORTED_OS = ['linux', 'darwin', 'win32'] as const

export const SUPPORTED_TARGETS =
  'linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, darwin-x64, darwin-arm64, win32-x64, win32-arm64'

export interface ParsedTarget {
  raw: string
  platform: string
  arch: string
  libc?: string
}

// Parsed triplet must match this shape exactly. We anchor and constrain each
// segment so that inputs like `linux-x64-musl-../../outside` are rejected
// outright — otherwise `target.raw` would later flow into path.join for the
// output directory and could escape it.
const TARGET_PATTERN = /^(linux|darwin|win32)-(x64|arm64)(?:-(musl))?$/

export function parseTarget (raw: string): ParsedTarget {
  const match = TARGET_PATTERN.exec(raw)
  if (!match) {
    throw new PnpmError('PACK_APP_INVALID_TARGET',
      `Invalid target: "${raw}". Expected format: <os>-<arch>[-<libc>] where <os> is ${SUPPORTED_OS.join('|')}, <arch> is x64|arm64, optional <libc> is musl (linux only).`)
  }
  const [, platform, arch, libc] = match
  if (libc === 'musl' && platform !== 'linux') {
    throw new PnpmError('PACK_APP_INVALID_TARGET',
      `The "musl" libc suffix is only valid for linux targets (got "${raw}").`)
  }
  return { raw, platform, arch, libc: libc || undefined }
}
