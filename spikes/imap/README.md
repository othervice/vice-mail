# Spike A: IMAP library

Throwaway code for stage 0. It checks `async-imap` 0.12 with Tokio and rustls against the local
Stalwart test server. Product code never imports it. The decision and all measured numbers are in
`docs/decisions/0002-imap-bibliothek.md`.

## Run

```
./run-all.sh        # reset + seed 10000 (INBOX ~9959), bench, all scenarios, shutdown, unit tests, OpenSSL check;
                    # leaves the test server stopped, writes results/*.txt
```

Single steps (test server running: `scripts/testmail.sh start`, seeded with
`scripts/testmail.sh seed 10000 --filler-folder INBOX`):

```
cargo build --release
./target/release/imap-spike all                 # every scenario below, one PASS/FAIL line per check
./target/release/imap-spike <scenario>          # tls starttls caps folders select fetch condstore idle
                                                # move append keywords reconnect smtp
./target/release/imap-spike shutdown            # stops and restarts the test server during IDLE/FETCH
./target/release/imap-spike bench [--windows 100,500,2000] [--runs 5]
cargo test --release                            # modified UTF-7 unit tests
cargo tree -i openssl-sys                       # must report "did not match any packages"
```

Exit status is 0 only if every check passed. Scenarios create and delete folders under `Spike/`.
The CONDSTORE and IDLE scenarios add a message to alice's INBOX and change flags on one message
(they are restored).

## Layout

| File | Content |
|---|---|
| `src/net.rs` | implicit TLS and STARTTLS connect, greeting, login |
| `src/tls.rs` | rustls configs: platform verifier plus extra roots (test CA), webpki-only |
| `src/timeout_io.rs` | read/write inactivity timeouts around the stream (async-imap has none) |
| `src/errors.rs` | typed `NetError` (CertUntrusted, AuthFailed, Timeout, ConnectionLost, ReadOnly, …) |
| `src/raw.rs` | raw commands with tagged status and response codes (APPENDUID, COPYUID, MODIFIED, READ-ONLY), checked UID FETCH |
| `src/mutf7.rs` | modified UTF-7 mailbox names |
| `src/proxy.rs` | fault-injecting TCP proxy (close, stall, refuse) |
| `src/scenarios.rs` | one function per requirement |
| `src/bench.rs` | header fetch timing, memory, wire bytes |
| `src/smtp.rs` | lettre 0.11 on 1587 with the same extra-root concept |

## Dependencies

async-imap 0.12.0, tokio 1.54.1, tokio-rustls 0.26.6, rustls 0.23.46 (ring provider),
rustls-platform-verifier 0.7.1, lettre 0.11.24, futures 0.3.34, thiserror 2.0.21, anyhow 1.0.104,
libc 0.2.191 (getrusage in the benchmark only). All MIT and/or Apache-2.0. ring is Apache-2.0 AND ISC.
No OpenSSL, no native-tls, no aws-lc-sys.
