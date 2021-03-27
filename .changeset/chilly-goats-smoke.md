---
"pnpm": major
---

pnpx does not automatically install packages. A prompt asks the user if a package should be installed, if it is not present.

`pnpx --yes` tells pnpx to install any missing package.

`pnpx --no` makes pnpx fail if the called packages is not installed.
