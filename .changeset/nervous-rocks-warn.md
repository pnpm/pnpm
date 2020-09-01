---
"@pnpm/plugin-commands-script-runners": major
---

The start and stop script commands are removed.
There is no reason to define separate handlers for shorthand commands
as any unknown command is automatically converted to a script.
