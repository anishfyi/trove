//! Library surface of the `trove` CLI. The binary in `main.rs` and the MCP
//! server in `mcp.rs` share these modules so both front doors use one
//! implementation.

pub mod ask;
pub mod llm;
pub mod mcp;
pub mod paths;
pub mod remember;
pub mod repl;
pub mod tools;
