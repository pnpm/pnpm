import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

// Junctions need no privileges on Windows and store absolute targets
// natively; the type is ignored on POSIX.
const SYMLINK_TYPE = 'junction'

/**
 * Creates a symlink whose stored target is the absolute `target` path, unlike
 * `symlink-dir` which always relativizes. Used for links into externally
 * materialized package directories (e.g. Nix store paths): the target never
 * moves, while the project directory may, so a relative link would break.
 *
 * With `overwrite` (the default) an existing symlink at `link` is replaced
 * unless it already points at `target`; without it, an existing file surfaces
 * as `EEXIST` so the caller can decide.
 */
export async function forceAbsoluteSymlink (
  target: string,
  link: string,
  opts?: { overwrite?: boolean }
): Promise<{ reused: boolean }> {
  try {
    await fs.symlink(target, link, SYMLINK_TYPE)
    return { reused: false }
  } catch (err: unknown) {
    if (!util.types.isNativeError(err) || !('code' in err)) throw err
    if (err.code === 'ENOENT') {
      await fs.mkdir(path.dirname(link), { recursive: true })
      await fs.symlink(target, link, SYMLINK_TYPE)
      return { reused: false }
    }
    if (err.code !== 'EEXIST' || opts?.overwrite === false) throw err
  }
  let existingTarget: string | undefined
  try {
    existingTarget = await fs.readlink(link)
  } catch {
    // Not a symlink — a real file or directory occupies the spot.
  }
  if (existingTarget === target) return { reused: true }
  if (existingTarget != null) {
    await fs.unlink(link)
  } else {
    await fs.rm(link, { recursive: true, force: true })
  }
  await fs.symlink(target, link, SYMLINK_TYPE)
  return { reused: false }
}
