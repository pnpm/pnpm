---
"pnpm": minor
---

An install lists the lifecycle scripts it ran in the checks footer above `Done in`. One line names the project scripts that ran, one the dependencies that were built, and one the dependency builds that came from the side-effects cache. A dependency that was skipped because its optional build failed is listed as a problem.

In a terminal, the footer is shown live below the progress while the install runs. While scripts run, their line shows a spinner and names the running scripts. A script that fails prints its output, as before. In CI and piped output, a script that succeeds without printing anything no longer prints any lines.
