# DISM Pro

A clean-room, open-source successor to [Dism++](https://github.com/Chuyu-Team/Dism-Multi-language):
a tiny, portable Windows servicing tool for the running OS, offline installs and WIM/ESD/VHD images.
It runs inside WinPE.

**What's new vs. Dism++:** every change is previewed before it's applied, journaled, and undoable
where Windows allows it. There's a scriptable CLI with JSON output, declarative recipes, an Image
Workbench pipeline, a health & repair center with decoded CBS logs, and a modern accessible UI. It also has a
Geek Uninstaller-style uninstaller with undoable leftover removal and install tracking, evidence-based registry cleanup,
and an MCP server so AI agents can plan changes that a human approves.

Licensed under [Apache-2.0](LICENSE). Not affiliated with Microsoft or Chuyu Team.

> Status: **Phase 0 complete (2026-10-15); Phase 1 (toolchain & spikes) next.** No code yet. See [docs/03-roadmap.md](docs/03-roadmap.md).

- Written in Rust ([ADR-0001](docs/adr/0001-language-rust.md))
- Built, tested and benchmarked **only in Docker** ([ADR-0003](docs/adr/0003-docker-only-toolchain.md))
- Start with [docs/README.md](docs/README.md)
