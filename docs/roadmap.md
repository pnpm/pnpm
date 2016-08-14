# Roadmap

`pnpm` will stay in `<1.0.0` until it's achieved feature parity with `npm install`.

- [ ] `pnpm install`
  - [x] npm packages
  - [x] install from packages (`npm i`)
  - [x] @scoped packages (`npm i @rstacruz/tap-spec`)
  - [x] tarball release packages (`npm i http://foo.com/tar.tgz`)
  - [x] compiled packages (`npm i node-sass`)
  - [x] bundled dependencies (`npm i fsevents@1.0.6`)
  - [ ] git-hosted packages (`npm i rstacruz/scourjs`)
  - [x] optional dependencies (`npm i escodegen@1.8.0` wants `source-map@~0.2.0`)
  - [x] file packages (`npm i file:../path`)
  - [x] windows support
  - [x] bin executables
  - [x] `--global` installs
  - [x] `--save` (et al)
- [x] `pnpm uninstall`
- [x] `pnpm ls`
