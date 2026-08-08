---
"get-pnpm": minor
---

New package for installing pnpm with a single command, without piping a script into a shell:

```sh
npx get-pnpm         # the latest version
npx get-pnpm 12      # a specific major
npx get-pnpm 11.20.0 # an exact version, or a dist-tag such as next-12
```

It installs the same standalone executable that https://get.pnpm.io/install.sh does, and finishes by running `pnpm setup`, so pnpm ends up in `PNPM_HOME` and on your `PATH` with no dependency on Node.js afterwards. The executable is downloaded from the registry npm is configured with (`npm_config_registry`) rather than from GitHub, and its checksum is verified against the registry metadata.
