import * as esbuild from 'esbuild';

esbuild
  .build({
    entryPoints: ['../worker/lib/worker.js'],
    bundle: true,
    platform: 'node',
    outfile: 'dist/worker.js',
    loader: {
      '.node': "copy",
    },
  })
  .catch((e: any) => {
    console.error(e)
    process.exit(1)
  })

