---
id: pnpm-server
title: pnpm server
original_id: pnpm-server
---

## pnpm server start

Added in: v1.30.0

Starts a server that does all interactions with the store.
Other commands will delegate any store-related tasks to this server.

### background

Added in: v1.30.0

* Default: **false**
* Type: **Boolean**

Runs the server in the background.

### protocol

Added in: v1.30.0

* Default: **auto**
* Type: **auto**, **tcp**, **ipc**

The communication protocol used by the server.
When **auto** is used, **ipc** used on non-Windows servers and **tcp** on Windows.

### port

Added in: v1.30.0

* Default: **5813**
* Type: **port number**

The port number to use, when TCP is used for communication.
If port is specified and **protocol** is set to auto, **tcp** protocol is used.

Other configs that are used by `pnpm server`: **store**, **lock**.

### ignore-stop-requests

Added in: v1.30.0

* Default: **false**
* Type: **Boolean**

Disallows stopping the server using `pnpm server stop`.

### ignore-upload-requests

Added in: v1.31.0

* Default: **false**
* Type: **Boolean**

Disallows creating new side effect cache during install.

## pnpm server stop

Added in: v1.30.0

Stops the store server.

## pnpm server status

Added in: v2.5.0

Prints information about the running server.
