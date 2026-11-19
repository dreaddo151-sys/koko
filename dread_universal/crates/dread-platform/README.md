# dread-platform

Read-only, cross-platform host and runtime discovery.

The crate detects OS, architecture, logical CPU threads, NVIDIA/AMD/Apple GPU metadata where known utilities are available, and a local llama.cpp server executable. It invokes only fixed utility names directly—never through a shell—and does not install drivers, modify the host, or launch an inference server.

Detection is best-effort. Missing utilities produce an empty capability field, not a fabricated device. GPU runtime ownership and model loading remain separate responsibilities.
