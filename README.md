# LocalBoard

**Ask your data questions in plain language. A private AI data analyst that runs on your computer.**

[![Watch the LocalBoard demo](https://img.youtube.com/vi/PAbbz2CYxac/maxresdefault.jpg)](https://youtu.be/PAbbz2CYxac)

- **Ask:** connect CSV, Excel, Parquet or JSON files, or a PostgreSQL, MySQL or SQLite database (other connectors are experimental and hidden), and ask in plain English. The analyst writes the SQL, runs it, checks it and answers with a chart.
- **Build:** turn answers into dashboards, multi-page datasites, PDF reports and slide decks.
- **Automate:** run checks on a schedule, when a file changes or on demand: thresholds, anomaly alerts, comparisons and scheduled reports.
- **Control it:** a SQL editor (Query Bench), analysts with their own rules and examples, an audit log, column masking and an MCP server for Claude Desktop and Cursor.

Queries run on your computer. AI runs through a provider you configure (Anthropic, OpenAI, Gemini, Ollama or an OpenAI-compatible endpoint); cloud providers receive your questions, schema, sample rows and results, and Offline mode blocks them. Other outbound traffic is what you set up (database hosts, optional SMTP or remote MCP). Every query is checked to be a single read-only `SELECT`.

**[⬇ Download for macOS](https://github.com/aryan2621/localboard/releases/latest)**
· macOS 11+, Apple silicon · [Windows and all downloads](https://github.com/aryan2621/localboard/releases/latest)

🛠 **[Developer guide](docs/dev.md)**: every feature in detail, build from source, privacy and governance

## Install

1. Open the `.dmg` and drag **LocalBoard** into **Applications**, then open it.
2. If macOS says **"Apple could not verify LocalBoard is free of malware"**: click **Done**, go to
   **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. (LocalBoard isn't
   signed with a paid Apple certificate; that's the only reason for the warning.)
3. If it says LocalBoard **"is damaged"**, run this once in Terminal and open it again:
   ```bash
   xattr -dr com.apple.quarantine /Applications/LocalBoard.app
   ```
4. Pick an AI provider in Settings, AI (optional; without one you get SQL-only mode). Click **Load sample data** to try it without your own data.

## Quick start

1. **Connections:** add a file, a folder or a database, or load the sample data.
2. **New chat:** ask *"Why is the West region down last month?"* or pick an output: Answer, Dashboard, Datasite, PDF report or Presentation.
3. **Show SQL** lists every query the analyst ran. **⌘K** searches everything.

## Build from source

```bash
git clone https://github.com/aryan2621/localboard.git
cd localboard && npm install && npm run tauri dev
```

Needs Node 20+, Rust, CMake (`brew install cmake`) and the Tauri prerequisites. Details in the [developer guide](docs/dev.md).
