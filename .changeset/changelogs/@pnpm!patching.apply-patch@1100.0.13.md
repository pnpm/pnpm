## 1100.0.13

### Patch Changes

- Fixed several bugs in applying patches:

  - Deleting a file whose path contains ` b/` no longer deletes a different file.
  - A hunk without context lines now inserts its lines after the line its header names.
  - A patch that creates a file no longer overwrites an existing file with different content. pnpm reports the patch as failed.

- Updated dependencies:
  - @pnpm/error@1100.2.3
