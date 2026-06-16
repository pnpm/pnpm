---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Fixed the progress line showing leftover characters from external processes that write to the terminal between progress updates (e.g. an SSH passphrase prompt would leave a fragment like `added 0sa':`). The interactive reporter now redraws each frame in place, erasing to the end of the display before reprinting, so any such remnants are cleared [#12350](https://github.com/pnpm/pnpm/issues/12350).
