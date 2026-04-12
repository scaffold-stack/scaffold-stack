# stacksdapp-parser

The core ABI extraction engine for the `stacksdapp` ecosystem.

This crate provides a robust parser for Clarity smart contracts. It scans `.clar` files and extracts high-level Abstract Binary Interfaces (ABIs), including function signatures, argument types, and contract metadata.

### Features
* **Deep Parsing:** Accurate extraction of Clarity syntax.
* **Type Mapping:** Support for all Stacks native types (uint, principal, response, etc.).
* **Integration Ready:** Optimized for use in code generation pipelines.