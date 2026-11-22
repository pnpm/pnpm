import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const REPO_ROOT = path.join(import.meta.dirname, '..', '..', '..', '..')

/**
 * The `@pnpm/pnpr` version the catalog pins. `pnpr-prepare` writes that
 * server's own storage format, so it is built from the same release.
 */
export function pnprVersion () {
  const catalog = readFileSync(path.join(REPO_ROOT, '.config', 'pnpm', 'catalog.yaml'), 'utf8')
  const version = /^ {2}'@pnpm\/pnpr': (\S+)$/m.exec(catalog)?.[1]
  if (version == null) throw new Error('The catalog does not pin @pnpm/pnpr')
  return version
}

/** Where `pnpm run install:pnpr-prepare` puts `pnpr-prepare` for the pinned pnpr release. */
export function pnprPrepareBinPath () {
  const ext = process.platform === 'win32' ? '.exe' : ''
  return path.join(REPO_ROOT, '.pnpr-prepare', pnprVersion(), 'bin', `pnpr-prepare${ext}`)
}

function installPnprPrepare () {
  const bin = pnprPrepareBinPath()
  if (existsSync(bin)) return
  const version = pnprVersion()
  const result = spawnSync('cargo', [
    'install',
    '--git', 'https://github.com/pnpm/pnpm',
    '--tag', `pnpr@${version}`,
    '--locked',
    '--root', path.dirname(path.dirname(bin)),
    'pnpr-fixtures',
    '--bin', 'pnpr-prepare',
  ], { stdio: 'inherit' })
  if (result.status !== 0) {
    throw new Error(`cargo install of pnpr-prepare ${version} failed (exit ${result.status ?? result.signal})`)
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  installPnprPrepare()
}
