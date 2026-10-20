# SKY Support server (hbbs)

This is the ID/rendezvous server of **SKY Support**, the remote-support client of [SKY](https://sky.bg)
(fork: <https://github.com/skycobg/SKY-Support>). It is a fork of
[rustdesk-server](https://github.com/rustdesk/rustdesk-server) 1.1.16 and is licensed, like it, under the
**GNU AGPL-3.0** (see `LICENSE`). Our changes live on the `sky` branch; everything else is upstream.

## What we changed (branch `sky`)

- **Change of ID** (`src/rendezvous_server.rs` `change_id`, `src/peer.rs` `PeerMap::change_id`): the client's
  "Change ID" and `--set-id` send `RegisterPk{old_id, id, uuid}` over TCP; upstream OSS answers `NOT_SUPPORT`.
  We rename the peer (same database row, uuid and public key) when the request carries the uuid stored for
  `old_id`, comes from the public IP of its last registration and not over WebSocket, and the new ID is valid
  (`^[a-zA-Z][\w-]{5,15}$`) and free; otherwise `UUID_MISMATCH`, `NOT_SUPPORT`, `INVALID_ID_FORMAT`,
  `ID_EXISTS` or `TOO_FREQUENT`. The old ID stays reserved for the same device.
- Build: `.github/workflows/sky-build.yml` (hbbs, x86_64 Linux, static musl).

No keys, passwords or addresses of SKY are in this repository.
