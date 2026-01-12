import { build } from 'esbuild'

/**
 * We publish the "pnpm" package bundled with all refclone artifacts for macOS and Windows.
 * Unfortunately, we need to do this because otherwise corepack wouldn't be able to install
 * pnpm with reflink support. Reflink is only unpacking the pnpm tarball and does no additional actions.
 */
;(async () => {
  try {
    const banner = { js: `import { createRequire as _cr } from 'module';const require = _cr(import.meta.url); const __filename = import.meta.filename; const __dirname = import.meta.dirname` }
    await build({
      entryPoints: ['lib/pnpm.js'],
      bundle: true,
      platform: 'node',
      outfile: 'dist/pnpm.mjs',
      // Create separate source map files, but intentionally omit
      // sourceMappingUrl comment links from the original *.mjs/*.cjs files. The
      // resulting *.map file is also excluded from NPM publishing.
      //
      // This keeps pnpm distributions small, but still allows developers to use
      // .map files to reverse minified stack traces using tools such as
      // https://sourcemap.tools. For more examples, see
      // https://github.com/pnpm/pnpm/pull/10378
      sourcemap: 'external',
      format: 'esm',
      banner,
      external: [
        'node-gyp',
        './get-uid-gid.js', // traces back to: https://github.com/npm/uid-number/blob/6e9bdb302ae4799d05abf12e922ccdb4bd9ea023/uid-number.js#L31
      ],
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

    await build({
      entryPoints: ['../worker/lib/worker.js'],
      bundle: true,
      platform: 'node',
      sourcemap: 'external',
      format: 'esm',
      outfile: 'dist/worker.js',
      banner,
      loader: {
        '.node': 'copy',
      },
    })
  } catch (err) {
    console.error(err)
    process.exit(1)
  }
})()
