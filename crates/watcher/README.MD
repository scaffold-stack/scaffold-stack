# stacksdapp-watcher

Automated file-system monitoring for Stacks projects.

A specialized utility that tracks changes in your Clarity contracts. Upon saving a contract, it triggers a re-generation of your frontend types, providing a seamless "Hot Reload" experience for blockchain logic.

### Features
* **Debounced Events:** Prevents redundant builds during rapid saving.
* **Low Overhead:** Optimized for minimal CPU usage.
* **Dev-Ex Focused:** A core component of the `stacksdapp dev` workflow.