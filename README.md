# Sessions Manager

A lightweight, high-performance command-line interface written in Rust to list, query, and inspect active/past AI agent session history across multiple providers.

It currently aggregates, parses, and filters session logs for:
- **Claude CLI**: `~/.claude/projects/`
- **Gemini / Antigravity CLI**: `~/.gemini/antigravity-cli/brain/`
- **Old Gemini CLI**: `~/.gemini/tmp/`
- **Codex**: `~/.codex/sessions/`
- **Pi**: `~/.pi/agent/sessions/`

## Features

- **Cross-Provider Aggregation**: Instantly index agent session files across five different platforms.
- **Path-Scoped Listing**: Filter sessions to show only those matching the current directory or a specific project workspace.
- **File Touch Identification**: Find which sessions read, modified, or executed commands on a particular file (e.g., `sessions list main.py`).
- **Interactive Inspector**: View the chronological history of terminal commands executed and files touched within a specific session using `sessions show <session_id>`.
- **High Readability**: Bold, color-coded terminal outputs and cleaned, formatted timestamps.

---

## Installation

### Prerequisites
Make sure you have Rust and Cargo installed:
```bash
cargo --version
```

### Build from Source
Clone the repository (or navigate to the project directory) and build:
```bash
cargo build --release
```

### Install to Local Binaries
Copy the compiled release binary to your user local binary folder:
```bash
mkdir -p ~/.local/bin
cp target/release/sessions-manager ~/.local/bin/sessions
```
*(Ensure `~/.local/bin` is added to your environment `PATH` variable).*

---

## Usage

List sessions for the current directory:
```bash
sessions
```

List sessions for a specific directory:
```bash
sessions /path/to/project
```

Find sessions in the current directory that touched a specific file:
```bash
sessions main.rs
```

Show detailed information (including files touched and commands executed) for a specific session:
```bash
sessions show <session_id>
```

Include deeply nested subagent sessions in the listing:
```bash
sessions --include-subagents
```

Display help information:
```bash
sessions -h
```

---

## License
MIT License
