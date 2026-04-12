# stacksdapp-deployer

Contract deployment orchestration for the Stacks blockchain.

This crate handles the complexities of getting Clarity code on-chain. It manages transaction broadcasting, fee estimation, and state tracking for various Stacks network environments.

### Features
* **Multi-Contract Support:** Deploy complex, interdependent projects.
* **Environment Aware:** Native support for Devnet, Testnet, and Mainnet.
* **State Tracking:** Maintains records of deployed contract addresses and timestamps.