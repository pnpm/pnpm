---
"@pnpm/plugin-commands-script-runners": major
"pnpm": major
---

The `dlx` command should always resolve packages to their exact versions and use those exact versions to create a cache key. This way `dlx` will always install the newest versions of the directly requested packages.
