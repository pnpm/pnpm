---
"@pnpm/plugin-commands-store-inspecting": patch
"pnpm": patch
---

New commands added for inspecting the store:

* **pnpm cat-index**: Prints the index file of a specific package in the store. The package is specified by its name and version: `pnpm cat-index <pkg name>@<pkg version>`
* **pnpm cat-file**: Prints the contents of a file based on the hash value stored in the index file. For example:
  ```
  pnpm cat-file sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
  ```
* **pnpm find-hash**: Lists the packages that include the file with the specified hash. For example:
  ```
  pnpm find-hash sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
  ```
  This command is **experimental**. We might change how it behaves.

Related issue: [#7413](https://github.com/pnpm/pnpm/issues/7413).
