---
"pnpm": minor
---

An install lists the dependencies that don't support the current Node.js version or platform in the checks footer above `Done in`. Each line says what the packages want, such as `✗ 15 packages want Node.js ^18.18.0 || ^20.9.0 || >=21.1.0, not 12.0.0: …`. When packages declare different Node.js ranges, the line shows the versions that satisfy all of them. pnpm no longer prints a warning for each of these packages.

The `pnpm:install-check` log event of an unsupported engine or platform now carries the error `code`, the `pkgId`, and what the package wants in `wanted`. An unsupported engine event also carries the `current` versions.
