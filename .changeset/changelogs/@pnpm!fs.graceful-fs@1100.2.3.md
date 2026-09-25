## 1100.2.3

### Patch Changes

- `pnpm install` in WSL now waits out Windows file locks on a Windows drive such as `/mnt/c`, as it already does on Windows. Before, an antivirus or indexer scan holding a file open could fail the install with `EACCES` [pnpm/pnpm#6155](https://github.com/pnpm/pnpm/issues/6155).

- On Windows, pnpm now retries saving `pnpm-lock.yaml` for up to a minute while another process holds the file open. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).
