---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Fixed the progress line showing leftover characters from external processes that write to the terminal between progress updates (e.g. an SSH passphrase prompt would leave a fragment like `added 0sa':`). Each rendered line is now followed by an ANSI "erase to end of line" sequence so any remnants are cleared [#12350](https://github.com/pnpm/pnpm/issues/12350).
