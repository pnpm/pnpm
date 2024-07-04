# @pnpm/plugin-commands-setup

## 5.1.0

### Minor Changes

- 3beb895: Bundled `pnpm setup` now creates `pnpx` script [#8230](https://github.com/pnpm/pnpm/issues/8230).

### Patch Changes

- @pnpm/cli-utils@3.1.3

## 5.0.10

### Patch Changes

- @pnpm/cli-utils@3.1.2

## 5.0.9

### Patch Changes

- @pnpm/cli-utils@3.1.1

## 5.0.8

### Patch Changes

- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/cli-utils@3.1.0

## 5.0.7

### Patch Changes

- @pnpm/cli-utils@3.0.7

## 5.0.6

### Patch Changes

- @pnpm/cli-utils@3.0.6

## 5.0.5

### Patch Changes

- @pnpm/cli-utils@3.0.5

## 5.0.4

### Patch Changes

- @pnpm/cli-utils@3.0.4

## 5.0.3

### Patch Changes

- @pnpm/cli-utils@3.0.3

## 5.0.2

### Patch Changes

- Updated dependencies [a80b539]
  - @pnpm/cli-utils@3.0.2

## 5.0.1

### Patch Changes

- @pnpm/cli-utils@3.0.1

## 5.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [43cdd87]
- Updated dependencies [3477ee5]
  - @pnpm/cli-utils@3.0.0

## 4.0.34

### Patch Changes

- @pnpm/cli-utils@2.1.9

## 4.0.33

### Patch Changes

- @pnpm/cli-utils@2.1.8

## 4.0.32

### Patch Changes

- @pnpm/cli-utils@2.1.7

## 4.0.31

### Patch Changes

- @pnpm/cli-utils@2.1.6

## 4.0.30

### Patch Changes

- @pnpm/cli-utils@2.1.5

## 4.0.29

### Patch Changes

- @pnpm/cli-utils@2.1.4

## 4.0.28

### Patch Changes

- @pnpm/cli-utils@2.1.3

## 4.0.27

### Patch Changes

- @pnpm/cli-utils@2.1.2

## 4.0.26

### Patch Changes

- @pnpm/cli-utils@2.1.1

## 4.0.25

### Patch Changes

- 34654835d: `pnpm setup` should add a newline at the end of the updated shell config file [#7227](https://github.com/pnpm/pnpm/issues/7227).
- Updated dependencies [43ce9e4a6]
  - @pnpm/cli-utils@2.1.0

## 4.0.24

### Patch Changes

- @pnpm/cli-utils@2.0.24

## 4.0.23

### Patch Changes

- @pnpm/cli-utils@2.0.23

## 4.0.22

### Patch Changes

- @pnpm/cli-utils@2.0.22

## 4.0.21

### Patch Changes

- @pnpm/cli-utils@2.0.21

## 4.0.20

### Patch Changes

- @pnpm/cli-utils@2.0.20

## 4.0.19

### Patch Changes

- @pnpm/cli-utils@2.0.19

## 4.0.18

### Patch Changes

- @pnpm/cli-utils@2.0.18

## 4.0.17

### Patch Changes

- @pnpm/cli-utils@2.0.17

## 4.0.16

### Patch Changes

- @pnpm/cli-utils@2.0.16

## 4.0.15

### Patch Changes

- @pnpm/cli-utils@2.0.15

## 4.0.14

### Patch Changes

- @pnpm/cli-utils@2.0.14

## 4.0.13

### Patch Changes

- 5b49c92e9: `pnpm setup` prints more details when it cannot detect the active shell.
  - @pnpm/cli-utils@2.0.13

## 4.0.12

### Patch Changes

- @pnpm/cli-utils@2.0.12

## 4.0.11

### Patch Changes

- @pnpm/cli-utils@2.0.11

## 4.0.10

### Patch Changes

- @pnpm/cli-utils@2.0.10

## 4.0.9

### Patch Changes

- @pnpm/cli-utils@2.0.9

## 4.0.8

### Patch Changes

- Updated dependencies [ee429b300]
  - @pnpm/cli-utils@2.0.8

## 4.0.7

### Patch Changes

- @pnpm/cli-utils@2.0.7

## 4.0.6

### Patch Changes

- @pnpm/cli-utils@2.0.6

## 4.0.5

### Patch Changes

- @pnpm/cli-utils@2.0.5

## 4.0.4

### Patch Changes

- @pnpm/cli-utils@2.0.4

## 4.0.3

### Patch Changes

- @pnpm/cli-utils@2.0.3

## 4.0.2

### Patch Changes

- @pnpm/cli-utils@2.0.2

## 4.0.1

### Patch Changes

- @pnpm/cli-utils@2.0.1

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/cli-utils@2.0.0

## 3.0.42

### Patch Changes

- @pnpm/cli-utils@1.1.7

## 3.0.41

### Patch Changes

- @pnpm/cli-utils@1.1.6

## 3.0.40

### Patch Changes

- @pnpm/cli-utils@1.1.5

## 3.0.39

### Patch Changes

- @pnpm/cli-utils@1.1.4

## 3.0.38

### Patch Changes

- @pnpm/cli-utils@1.1.3

## 3.0.37

### Patch Changes

- d80661d42: The configuration added by `pnpm setup` should check if the pnpm home directory is already in the PATH before adding to the PATH.

  Before this change, this code was added to the shell:

  ```sh
  export PNPM_HOME="$HOME/Library/pnpm"
  export PATH="$PNPM_HOME:$PATH"
  ```

  Now this will be added:

  ```sh
  export PNPM_HOME="$HOME/Library/pnpm"
  case ":$PATH:" in
    *":$PNPM_HOME:"*) ;;
    *) export PATH="$PNPM_HOME:$PATH" ;;
  esac
  ```

- Updated dependencies [7d64d757b]
  - @pnpm/cli-utils@1.1.2

## 3.0.36

### Patch Changes

- @pnpm/cli-utils@1.1.1

## 3.0.35

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/cli-utils@1.1.0

## 3.0.34

### Patch Changes

- @pnpm/cli-utils@1.0.34

## 3.0.33

### Patch Changes

- @pnpm/cli-utils@1.0.33

## 3.0.32

### Patch Changes

- @pnpm/cli-utils@1.0.32

## 3.0.31

### Patch Changes

- @pnpm/cli-utils@1.0.31

## 3.0.30

### Patch Changes

- @pnpm/cli-utils@1.0.30

## 3.0.29

### Patch Changes

- @pnpm/cli-utils@1.0.29

## 3.0.28

### Patch Changes

- @pnpm/cli-utils@1.0.28

## 3.0.27

### Patch Changes

- @pnpm/cli-utils@1.0.27

## 3.0.26

### Patch Changes

- @pnpm/cli-utils@1.0.26

## 3.0.25

### Patch Changes

- @pnpm/cli-utils@1.0.25

## 3.0.24

### Patch Changes

- @pnpm/cli-utils@1.0.24

## 3.0.23

### Patch Changes

- @pnpm/cli-utils@1.0.23

## 3.0.22

### Patch Changes

- @pnpm/cli-utils@1.0.22

## 3.0.21

### Patch Changes

- @pnpm/cli-utils@1.0.21

## 3.0.20

### Patch Changes

- @pnpm/cli-utils@1.0.20

## 3.0.19

### Patch Changes

- @pnpm/cli-utils@1.0.19

## 3.0.18

### Patch Changes

- @pnpm/cli-utils@1.0.18

## 3.0.17

### Patch Changes

- @pnpm/cli-utils@1.0.17

## 3.0.16

### Patch Changes

- @pnpm/cli-utils@1.0.16

## 3.0.15

### Patch Changes

- @pnpm/cli-utils@1.0.15

## 3.0.14

### Patch Changes

- @pnpm/cli-utils@1.0.14

## 3.0.13

### Patch Changes

- @pnpm/cli-utils@1.0.13

## 3.0.12

### Patch Changes

- @pnpm/cli-utils@1.0.12

## 3.0.11

### Patch Changes

- @pnpm/cli-utils@1.0.11

## 3.0.10

### Patch Changes

- @pnpm/cli-utils@1.0.10

## 3.0.9

### Patch Changes

- @pnpm/cli-utils@1.0.9

## 3.0.8

### Patch Changes

- @pnpm/cli-utils@1.0.8

## 3.0.7

### Patch Changes

- @pnpm/cli-utils@1.0.7

## 3.0.6

### Patch Changes

- @pnpm/cli-utils@1.0.6

## 3.0.5

### Patch Changes

- @pnpm/cli-utils@1.0.5

## 3.0.4

### Patch Changes

- @pnpm/cli-utils@1.0.4

## 3.0.3

### Patch Changes

- @pnpm/cli-utils@1.0.3

## 3.0.2

### Patch Changes

- @pnpm/cli-utils@1.0.2

## 3.0.1

### Patch Changes

- @pnpm/cli-utils@1.0.1

## 3.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/cli-utils@1.0.0

## 2.0.45

### Patch Changes

- @pnpm/cli-utils@0.7.43

## 2.0.44

### Patch Changes

- @pnpm/cli-utils@0.7.42

## 2.0.43

### Patch Changes

- @pnpm/cli-utils@0.7.41

## 2.0.42

### Patch Changes

- @pnpm/cli-utils@0.7.40

## 2.0.41

### Patch Changes

- @pnpm/cli-utils@0.7.39

## 2.0.40

### Patch Changes

- @pnpm/cli-utils@0.7.38

## 2.0.39

### Patch Changes

- @pnpm/cli-utils@0.7.37

## 2.0.38

### Patch Changes

- @pnpm/cli-utils@0.7.36

## 2.0.37

### Patch Changes

- @pnpm/cli-utils@0.7.35

## 2.0.36

### Patch Changes

- @pnpm/cli-utils@0.7.34

## 2.0.35

### Patch Changes

- @pnpm/cli-utils@0.7.33

## 2.0.34

### Patch Changes

- @pnpm/cli-utils@0.7.32

## 2.0.33

### Patch Changes

- @pnpm/cli-utils@0.7.31

## 2.0.32

### Patch Changes

- @pnpm/cli-utils@0.7.30

## 2.0.31

### Patch Changes

- @pnpm/cli-utils@0.7.29

## 2.0.30

### Patch Changes

- @pnpm/cli-utils@0.7.28

## 2.0.29

### Patch Changes

- 8cb47ac9d: `pnpm setup`: don't use `setx` to set env variables on Windows.
  - @pnpm/cli-utils@0.7.27

## 2.0.28

### Patch Changes

- fe53c2986: On POSIX `pnpm setup` should suggest users to source the config instead of restarting the terminal.

## 2.0.27

### Patch Changes

- @pnpm/cli-utils@0.7.26

## 2.0.26

### Patch Changes

- @pnpm/cli-utils@0.7.25

## 2.0.25

### Patch Changes

- @pnpm/cli-utils@0.7.24

## 2.0.24

### Patch Changes

- @pnpm/cli-utils@0.7.23

## 2.0.23

### Patch Changes

- @pnpm/cli-utils@0.7.22

## 2.0.22

### Patch Changes

- @pnpm/cli-utils@0.7.21

## 2.0.21

### Patch Changes

- @pnpm/cli-utils@0.7.20

## 2.0.20

### Patch Changes

- @pnpm/cli-utils@0.7.19

## 2.0.19

### Patch Changes

- @pnpm/cli-utils@0.7.18

## 2.0.18

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17

## 2.0.17

### Patch Changes

- @pnpm/cli-utils@0.7.16

## 2.0.16

### Patch Changes

- @pnpm/cli-utils@0.7.15

## 2.0.15

### Patch Changes

- @pnpm/cli-utils@0.7.14

## 2.0.14

### Patch Changes

- @pnpm/cli-utils@0.7.13

## 2.0.13

### Patch Changes

- @pnpm/cli-utils@0.7.12

## 2.0.12

### Patch Changes

- @pnpm/cli-utils@0.7.11

## 2.0.11

### Patch Changes

- @pnpm/cli-utils@0.7.10

## 2.0.10

### Patch Changes

- @pnpm/cli-utils@0.7.9

## 2.0.9

### Patch Changes

- @pnpm/cli-utils@0.7.8

## 2.0.8

### Patch Changes

- @pnpm/cli-utils@0.7.7

## 2.0.7

### Patch Changes

- e6a9f157d: `pnpm setup` should not fail on Windows if `PNPM_HOME` is not yet in the system registry [#4757](https://github.com/pnpm/pnpm/issues/4757)

## 2.0.6

### Patch Changes

- @pnpm/cli-utils@0.7.6

## 2.0.5

### Patch Changes

- 71c7ed998: `pnpm setup` should update the config of the current shell, not the preferred shell.
- 460ccf60e: fix: make `pnpm setup` free of garbled characters.
- 61d102a99: `pnpm setup` should not override the PNPM_HOME env variable on Windows, unless `--force` is used.
- 7c9362d3d: fix `pnpm setup` breaks %PATH% with non-ascii characters [#4698](https://github.com/pnpm/pnpm/issues/4698)
- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5

## 2.0.4

### Patch Changes

- @pnpm/cli-utils@0.7.4

## 2.0.3

### Patch Changes

- @pnpm/cli-utils@0.7.3

## 2.0.2

### Patch Changes

- @pnpm/cli-utils@0.7.2

## 2.0.1

### Patch Changes

- @pnpm/cli-utils@0.7.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/cli-utils@0.7.0

## 1.1.35

### Patch Changes

- @pnpm/cli-utils@0.6.50

## 1.1.34

### Patch Changes

- @pnpm/cli-utils@0.6.49

## 1.1.33

### Patch Changes

- @pnpm/cli-utils@0.6.48

## 1.1.32

### Patch Changes

- @pnpm/cli-utils@0.6.47

## 1.1.31

### Patch Changes

- @pnpm/cli-utils@0.6.46

## 1.1.30

### Patch Changes

- @pnpm/cli-utils@0.6.45

## 1.1.29

### Patch Changes

- @pnpm/cli-utils@0.6.44

## 1.1.28

### Patch Changes

- @pnpm/cli-utils@0.6.43

## 1.1.27

### Patch Changes

- @pnpm/cli-utils@0.6.42

## 1.1.26

### Patch Changes

- @pnpm/cli-utils@0.6.41

## 1.1.25

### Patch Changes

- @pnpm/cli-utils@0.6.40

## 1.1.24

### Patch Changes

- @pnpm/cli-utils@0.6.39

## 1.1.23

### Patch Changes

- @pnpm/cli-utils@0.6.38

## 1.1.22

### Patch Changes

- @pnpm/cli-utils@0.6.37

## 1.1.21

### Patch Changes

- @pnpm/cli-utils@0.6.36

## 1.1.20

### Patch Changes

- @pnpm/cli-utils@0.6.35

## 1.1.19

### Patch Changes

- @pnpm/cli-utils@0.6.34

## 1.1.18

### Patch Changes

- @pnpm/cli-utils@0.6.33

## 1.1.17

### Patch Changes

- b847e0300: `pnpm setup` should create shell rc files for pnpm path configuration if no such file exists prior [#4027](https://github.com/pnpm/pnpm/issues/4027).

## 1.1.16

### Patch Changes

- @pnpm/cli-utils@0.6.32

## 1.1.15

### Patch Changes

- @pnpm/cli-utils@0.6.31

## 1.1.14

### Patch Changes

- @pnpm/cli-utils@0.6.30

## 1.1.13

### Patch Changes

- @pnpm/cli-utils@0.6.29

## 1.1.12

### Patch Changes

- @pnpm/cli-utils@0.6.28

## 1.1.11

### Patch Changes

- @pnpm/cli-utils@0.6.27

## 1.1.10

### Patch Changes

- @pnpm/cli-utils@0.6.26

## 1.1.9

### Patch Changes

- @pnpm/cli-utils@0.6.25

## 1.1.8

### Patch Changes

- @pnpm/cli-utils@0.6.24

## 1.1.7

### Patch Changes

- @pnpm/cli-utils@0.6.23

## 1.1.6

### Patch Changes

- @pnpm/cli-utils@0.6.22

## 1.1.5

### Patch Changes

- 04b7f6086: Use safe-execa instead of execa to prevent binary planting attacks on Windows.

## 1.1.4

### Patch Changes

- @pnpm/cli-utils@0.6.21

## 1.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.20

## 1.1.2

### Patch Changes

- cee8b73f1: Set `PATH` environment variable to `PNPM_HOME` on Win32 platform

## 1.1.1

### Patch Changes

- @pnpm/cli-utils@0.6.19

## 1.1.0

### Minor Changes

- ade0fa92f: The original binary will not be removed after pnpm setup.

## 1.0.1

### Patch Changes

- @pnpm/cli-utils@0.6.18

## 1.0.0

### Major Changes

- 71cc21832: Print info message about the requirement to open a new terminal.

## 0.2.1

### Patch Changes

- @pnpm/cli-utils@0.6.17

## 0.2.0

### Minor Changes

- 8d038f8f1: pnpm setup moves the CLI to the pnpm home directory.

## 0.1.12

### Patch Changes

- @pnpm/cli-utils@0.6.16

## 0.1.11

### Patch Changes

- @pnpm/cli-utils@0.6.15

## 0.1.10

### Patch Changes

- @pnpm/cli-utils@0.6.14

## 0.1.9

### Patch Changes

- @pnpm/cli-utils@0.6.13

## 0.1.8

### Patch Changes

- @pnpm/cli-utils@0.6.12

## 0.1.7

### Patch Changes

- @pnpm/cli-utils@0.6.11

## 0.1.6

### Patch Changes

- @pnpm/cli-utils@0.6.10

## 0.1.5

### Patch Changes

- @pnpm/cli-utils@0.6.9

## 0.1.4

### Patch Changes

- @pnpm/cli-utils@0.6.8

## 0.1.3

### Patch Changes

- 6a64c1ff5: A summary should be printed.

## 0.1.2

### Patch Changes

- @pnpm/cli-utils@0.6.7

## 0.1.1

### Patch Changes

- @pnpm/cli-utils@0.6.6

## 0.1.0

### Minor Changes

- 473223be9: Project created.

### Patch Changes

- @pnpm/cli-utils@0.6.5
