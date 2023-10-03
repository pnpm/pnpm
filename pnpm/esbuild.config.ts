import { build } from 'esbuild'

;(async () => {
  try {
    await build({
      entryPoints: ['lib/pnpm.js'],
      bundle: true,
      platform: 'node',
      outfile: 'dist/pnpm.cjs',
      external: ['node-gyp'],
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
  } catch (err) {
    console.error(err)
    process.exit(1)
  }

  try {
    build({
      entryPoints: ['../worker/lib/worker.js'],
      bundle: true,
      platform: 'node',
      outfile: 'dist/worker.js',
      loader: {
        '.node': 'copy',
      },
    })
  } catch (err) {
    console.error(err)
    process.exit(1)
  }
})()
