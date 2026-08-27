---
"pacquet": patch
---

Fixed a large install-time regression on macOS for installs that rebuild `node_modules` from a warm store [#14231](https://github.com/pnpm/pnpm/issues/14231). APFS serializes file-cloning and hard-linking syscalls volume-wide, so importing packages one file at a time from many threads was bounded by a per-volume ceiling and got slower the more CPU cores the machine had. On macOS, `pnpm install` now materializes each package once into the store's `links` directory (the same canonical slots `enableGlobalVirtualStore` uses) and copies it into `node_modules/.pnpm` with a single copy-on-write directory clone per package, replacing tens of thousands of per-file syscalls with one per package. Applies with the default `nodeLinker: isolated` when `enableGlobalVirtualStore` is off and `packageImportMethod` is `auto`, `clone`, or `clone-or-copy`; hoisted, global-virtual-store, and explicit `hardlink`/`copy` installs are unchanged.
