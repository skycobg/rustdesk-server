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
  (6 to 16 of a-z, A-Z, 0-9, `-` and `_` in any order, so only digits or only letters too; upstream asks for a
  letter first: `is_valid_sky_id`) and free; otherwise `UUID_MISMATCH`, `NOT_SUPPORT`, `INVALID_ID_FORMAT`,
  `ID_EXISTS` or `TOO_FREQUENT`. The old ID stays reserved for the same device.
- **Key exchange on TCP** (`src/rendezvous_server.rs` `handle_listener_inner`, `send_to_sink`): like the Pro
  server, hbbs sends a `KeyExchange` with an ephemeral box key signed by the server key as the first message
  of every TCP connection on 21116. A client that is signed in to an API server waits for it and otherwise fails
  with "Failed to secure tcp: deadline has elapsed". When the client answers, both directions are sealed with
  the exchanged key. We advertise version 1 (one key per direction, derived as in the RustDesk 1.5.1 client,
  `kx_subkey`); a client that answers with version 0 (before 1.5.1) gets the original single key. Clients that
  do not answer stay in the clear and skip the message.
- Build: `.github/workflows/sky-build.yml` (hbbs, x86_64 Linux, static musl).

No keys, passwords or addresses of SKY are in this repository.
