import path from 'path'
import PnpmError from '@pnpm/error'
import type { FilesIndex } from '@pnpm/fetcher-base'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import preferredPM from 'preferred-pm'
import packlist from 'npm-packlist'

export async function runPrepareHook (pkgDir: string) {
  const manifest = await safeReadPackageFromDir(pkgDir)

  if (manifest?.scripts?.prepare != null && manifest.scripts.prepare !== '') {
    const pm = (await preferredPM(pkgDir))?.name ?? 'npm'
    try {
      await execa(pm, ['install'], { cwd: pkgDir })
    } catch (err: any) { // eslint-disable-line
      throw new PnpmError('PREPARE_PKG_FAILURE', err.shortMessage ?? err.message)
    }
    await rimraf(path.join(pkgDir, 'node_modules'))
  }
}

export async function filterFilesIndex (pkgDir: string, filesIndex: FilesIndex): Promise<FilesIndex> {
  const included = new Set(await packlist({ path: pkgDir }))
  const filteredIndex = {}

  for (const pathname in filesIndex) {
    if (included.has(pathname)) {
      filteredIndex[pathname] = filesIndex[pathname]
    }
  }
  return filteredIndex
}
