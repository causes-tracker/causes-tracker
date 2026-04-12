# tools/proto-gen

Rust binary that drives `tonic-prost-build` to generate gRPC server/client code from `.proto` definitions.
Separated from `//tools:proto_gen` (the shell wrapper) so that the Rust compilation is cached independently of the generation script.
