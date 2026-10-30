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

> Status: **early development.** The engine (plan model, write-ahead journal, undo), the Windows layer and a first CLI (`dismpro-cli`: targets, packages, features, health, reboot reasons, journal list/show/undo, purge) build and pass their tests, but haven't yet serviced a real Windows image. Not ready for use.

- Written in Rust
- Built, tested and benchmarked **only in Docker**: `docker compose run --rm <fmt|lint|test|test-win|bench|deny|build>`
