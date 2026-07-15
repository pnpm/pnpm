## 12.0.0-alpha.12

### Patch Changes

- The `--help` text now reads as user-facing help rather than developer documentation. Command and flag descriptions say what each option does for you, and the leftover markdown that was printing verbatim in the terminal — intra-doc links, an inline link, and an HTML-like path placeholder — has been cleaned out.

- Fixed installs failing on Windows with `ERROR_DIRECTORY` (os error 267) when re-linking over a global store whose directory junctions were restored in a dangling state (for example, a `store/v11` directory brought back by a CI cache, since tar can't round-trip a Windows reparse point). `create_dir_all` accepts such a junction because it keeps the directory attribute, but `CreateSymbolicLinkW` can't create a child link through it. The symlink writer now rebuilds the broken parent directory and retries instead of aborting the install.
