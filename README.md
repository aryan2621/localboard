# LocalBoard

**Ask your data questions in plain language. A private AI data analyst that runs on your computer.**

<!-- DEMO VIDEO: paste the https://github.com/user-attachments/assets/... link on the next line -->

- **Ask:** connect CSV, Excel, Parquet or JSON files, or a PostgreSQL, MySQL or SQLite database, and ask in plain English. The analyst writes the SQL, runs it, checks it and answers with a chart.
- **Build:** turn answers into dashboards, multi-page datasites, PDF reports and slide decks.
- **Automate:** run checks on a schedule, when a file changes or on demand: thresholds, anomaly alerts, comparisons and scheduled reports.
- **Control it:** a SQL editor (Query Bench), analysts with their own rules and examples, an audit log, column masking and an MCP server for Claude Desktop and Cursor.

Everything runs on your computer with a built-in local AI (llama.cpp). Your data never leaves it. Every query is checked to be a single read-only `SELECT`.

**[⬇ Download for macOS](https://github.com/aryan2621/localboard/releases/latest/download/LocalBoard_0.1.0_aarch64.dmg)**
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
4. The setup downloads an AI model (2.7 GB for 8 GB Macs, 5.7 GB for 16 GB or more, once). Click **Load sample data** to try it without your own data.

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
