import { build } from 'esbuild'
import path from 'path'

;(async () => {
  try {
    await buildAll({
      destDir: path.join(__dirname, 'dist'),
      external: ['@refclone/refclone'],
    })
    await buildAll({
      destDir: path.join(__dirname, 'dist_pkg'),
      external: [],
    })
  } catch (err) {
    console.error(err)
    process.exit(1)
  }
})()

async function buildAll ({ destDir, external }: { destDir: string, external: string[] }) {
  await build({
    entryPoints: ['lib/pnpm.js'],
    bundle: true,
    platform: 'node',
    outfile: path.join(destDir, 'pnpm.cjs'),
    external: ['node-gyp', ...external],
    define: {
      'process.env.npm_package_name': JSON.stringify(
        process.env.npm_package_name
      ),
      'process.env.npm_package_version': JSON.stringify(
        process.env.npm_package_version
      ),
    },
    loader: {
      '.node': 'copy',
    },
  })

  build({
    entryPoints: ['../worker/lib/worker.js'],
    bundle: true,
    platform: 'node',
    external,
    outfile: path.join(destDir, 'worker.js'),
    loader: {
      '.node': 'copy',
    },
  })
}
