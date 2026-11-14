# Vendored protos

Each crate's `build.rs` generates the Rust messages from these files.

## OpAMP

`opamp/v1/opamp.proto` and `opamp/v1/anyvalue.proto` are copied unchanged
from [opamp-spec](https://github.com/open-telemetry/opamp-spec) at tag
`v0.20.0` (commit `b08624151d000c1ca043b300e1f5e3bc93107987`), path
`proto/opamp/v1/`. They were taken from pg-yard at commit `60b8d2a`, which
vendors the same files unchanged.
