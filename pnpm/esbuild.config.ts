import * as esbuild from "esbuild";

esbuild
  .build({
    entryPoints: ["lib/pnpm.js"],
    bundle: true,
    platform: "node",
    outfile: "dist/pnpm.cjs",
    external: ["node-gyp"],
    define: {
      "process.env.npm_package_name": JSON.stringify(
        process.env.npm_package_name
      ),
      "process.env.npm_package_version": JSON.stringify(
        process.env.npm_package_version
      ),
    },
    loader: {
      ".node": "copy",
    },
  })
  .catch((e: any) => {
    console.error(e);
    process.exit(1);
  });
