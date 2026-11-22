---
name: testing-changes
description: Run the tests that cover a change to the TypeScript pnpm v11 CLI (pnpm11/), and recognize the cases where a scoped run passes without testing anything. Use whenever verifying a change before committing or pushing, or when deciding what to run after an edit.
---

# Testing a change

Run what the change affects. CI runs the full suite on three platforms for every pull request, so the local job is fast feedback, not a second gate.

```sh
pnpm --filter <package_name> test                                 # one package
pnpm --filter <package_name> test <file_path>                     # one file
pnpm --filter <package_name> test <file_path> -t <name_pattern>   # one case
```

The end-to-end tests in `pnpm11/pnpm/test/` run the **bundled** `pnpm11/pnpm/dist/pnpm.mjs`, not each package's `lib/`. After changing any TypeScript package, rebuild the bundle before running them:

```sh
pnpm --filter pnpm run compile
```

Skip that and the run tests the previous bundle, passing without ever touching your change.

The tests that use the test registry need `pnpr-prepare`. Build it once with `pnpm run install:pnpr-prepare`, which needs a Rust toolchain (`cargo`).

## Reporting

Name what you ran. "Ran the `@pnpm/lockfile.fs` tests plus `pnpm11/pnpm/test/install/misc.ts`; did not run the full suite" is an honest report. "Tests pass" after a single-package run is not.
