import fs, { promises as fsPromises } from 'fs'
import path from 'path'
import { getBinNodePaths } from '../src/getBinNodePaths.js'
import { temporaryDirectory } from 'tempy'

// On Windows, temporaryDirectory() may return 8.3 short paths (e.g., RUNNER~1)
// but getBinNodePaths resolves these via fs.promises.realpath (the native
// implementation that uses GetFinalPathNameByHandleW), returning long paths.
// Use the same fs.promises.realpath so expected and received paths match.
// Note: fs.realpathSync does NOT resolve 8.3 names — it uses a JS-only
// implementation that only resolves symlinks.
async function tmpdir (): Promise<string> {
  return fsPromises.realpath(temporaryDirectory())
}

test('returns package node_modules and sibling node_modules for virtual store layout', async () => {
  const tmp = await tmpdir()
  // Simulate: .pnpm/pkg@1.0.0/node_modules/pkg/bin/cli.js
  const binPath = path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'bin', 'cli.js')
  fs.mkdirSync(path.dirname(binPath), { recursive: true })
  fs.writeFileSync(binPath, '')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'node_modules'),
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules'),
  ])
})

test('returns only the node_modules dir when binary is directly inside node_modules/pkg', async () => {
  const tmp = await tmpdir()
  // Simulate: node_modules/pkg/bin/cli.js
  const binPath = path.join(tmp, 'node_modules', 'pkg', 'bin', 'cli.js')
  fs.mkdirSync(path.dirname(binPath), { recursive: true })
  fs.writeFileSync(binPath, '')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([
    path.join(tmp, 'node_modules', 'pkg', 'node_modules'),
    path.join(tmp, 'node_modules'),
  ])
})

test('returns empty array when there is no node_modules ancestor', async () => {
  const tmp = await tmpdir()
  // Simulate: some/path/bin/cli.js (no node_modules)
  const binPath = path.join(tmp, 'some', 'path', 'bin', 'cli.js')
  fs.mkdirSync(path.dirname(binPath), { recursive: true })
  fs.writeFileSync(binPath, '')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([])
})

test('resolves symlinks to find the real path', async () => {
  const tmp = await tmpdir()
  // Real location: .pnpm/pkg@1.0.0/node_modules/pkg/bin/cli.js
  const realBinDir = path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'bin')
  fs.mkdirSync(realBinDir, { recursive: true })
  fs.writeFileSync(path.join(realBinDir, 'cli.js'), '')

  // Symlink: node_modules/pkg -> .pnpm/pkg@1.0.0/node_modules/pkg
  const symlinkTarget = path.join(tmp, 'node_modules', 'pkg')
  fs.mkdirSync(path.join(tmp, 'node_modules'), { recursive: true })
  fs.symlinkSync(
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg'),
    symlinkTarget,
    'junction'
  )

  // Pass the symlinked path
  const binPath = path.join(symlinkTarget, 'bin', 'cli.js')
  const result = await getBinNodePaths(binPath)

  // Should resolve through the symlink and return paths based on the real location
  expect(result).toEqual([
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'node_modules'),
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules'),
  ])
})

test('falls back to original path when target directory does not exist', async () => {
  const tmp = await tmpdir()
  // Path that does not exist on disk
  const binPath = path.join(tmp, 'node_modules', 'pkg', 'bin', 'cli.js')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([
    path.join(tmp, 'node_modules', 'pkg', 'node_modules'),
    path.join(tmp, 'node_modules'),
  ])
})

test('handles scoped packages in virtual store layout', async () => {
  const tmp = await tmpdir()
  // Simulate: .pnpm/@scope+pkg@1.0.0/node_modules/@scope/pkg/bin/cli.js
  const binPath = path.join(tmp, '.pnpm', '@scope+pkg@1.0.0', 'node_modules', '@scope', 'pkg', 'bin', 'cli.js')
  fs.mkdirSync(path.dirname(binPath), { recursive: true })
  fs.writeFileSync(binPath, '')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([
    path.join(tmp, '.pnpm', '@scope+pkg@1.0.0', 'node_modules', '@scope', 'pkg', 'node_modules'),
    path.join(tmp, '.pnpm', '@scope+pkg@1.0.0', 'node_modules'),
  ])
})

test('binary at root of package (no subdirectory)', async () => {
  const tmp = await tmpdir()
  // Simulate: .pnpm/pkg@1.0.0/node_modules/pkg/cli.js (binary at package root)
  const binPath = path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'cli.js')
  fs.mkdirSync(path.dirname(binPath), { recursive: true })
  fs.writeFileSync(binPath, '')

  const result = await getBinNodePaths(binPath)

  expect(result).toEqual([
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules', 'pkg', 'node_modules'),
    path.join(tmp, '.pnpm', 'pkg@1.0.0', 'node_modules'),
  ])
})
