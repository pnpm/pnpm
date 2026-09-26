## 1100.0.32

### Patch Changes

- A script that pnpm runs without a terminal now ends when pnpm itself is killed. Killing pnpm's process group, as Playwright's `webServer` does to stop the command it started, used to leave the script running and holding the caller's output pipes open [#15555](https://github.com/pnpm/pnpm/issues/15555).

- Updated dependencies:
  - @pnpm/assert-project@1100.0.32
  - @pnpm/types@1102.1.1
