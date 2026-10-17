# LocalBoard developer guide

Local-first desktop AI data analyst: Tauri v2 + React + Rust + DuckDB + SQLite, with a bundled
llama.cpp server as the built-in AI (llama.cpp is the runtime by design; Ollama is not used). No cloud calls, no telemetry.

## Prerequisites

- Node.js 20+, npm, Rust stable, and the Tauri v2 platform prerequisites
  (macOS: Xcode command line tools; Linux: WebKit2GTK/GTK dev packages; Windows: Visual Studio C++ build tools and WebView2).
- macOS/Linux: **CMake** (`brew install cmake`). It builds the bundled `llama-server` once.

## Run

```sh
npm install
npm run tauri dev
```

The first run builds llama.cpp (pinned `v0.5.0`; macOS gets Metal) into `src-tauri/binaries/`, which takes
about a minute. Later runs reuse it. Windows downloads the official prebuilt CPU server (`b11205`) instead.
Tauri ships the binary inside the app, so end users install nothing.

For a release bundle: `npm run tauri build`.

## Models

Models are downloaded in the app (onboarding or Settings → AI models) as 4-bit GGUF files from Hugging
Face. Each download is pinned to a revision and verified against its SHA-256 before use.

| Model | Download | Needs RAM |
| --- | --- | --- |
| Qwen3.5 4B | 2.7 GB | 8 GB |
| Qwen3.5 9B | 5.7 GB | 16 GB |
| Gemma 4 12B | 6.7 GB | 16 GB |
| Gemma 4 26B A4B | 14.3 GB | 32 GB |
| Qwen3.8 27B | 16.5 GB | 32 GB |
| Nomic Embed Text v1.5 | 146 MB | 2 GB |

The app recommends one chat model based on detected memory. The embedding model is optional: with it, LocalBoard matches questions to the right analyst and to saved examples; without it, that matching uses keywords. Without a chat model, LocalBoard runs in SQL-only mode.
LocalBoard only uses models in its own models folder.

## Data

- Each workspace has its own SQLite file and DuckDB file under the app data directory.
- Files: CSV, TSV, Parquet, JSON/JSONL become DuckDB views over the original file. For XLSX, every
  populated sheet becomes its own table.
- Folders are watched; added, changed and removed files update the catalog automatically.
- Columns are profiled (empty %, distinct, range, top values), and likely foreign keys are detected from
  names plus value containment.
- Databases (PostgreSQL, MySQL, SQLite) open read-only. Passwords live in the OS keychain. For MySQL, use a
  SELECT-only account, because a read-only session does not prove the account itself is read-only.
  From Explore, **Add as live table** registers a read-only DuckDB scanner view (`postgres_scan` /
  `mysql_scan`, or SQLite `ATTACH … READ_ONLY`). First use may download a DuckDB extension. If that
  fails, LocalBoard says so and does not copy the table. Registered names go through the catalog; every
  query still hits the SQL guard. SQL Server is not in this build (sqlx 0.8 has no MSSQL driver).

## Asking questions

- **New chat** asks in plain language, or you can **Choose an output** (Answer, Dashboard, Datasite, PDF report, Presentation). The analyst picks the relevant tables, writes SQL with the local
  model, runs it, repairs it up to 3 times on errors, double-checks suspicious results, picks a chart and
  writes the answer from facts computed in code. Each step streams live, and **Stop** cancels at any point.
- "Why did X change" questions run a deterministic comparison of the two latest periods and rank the
  segments that drove the change.
- Forecast questions project the next periods in SQL (a seasonal baseline when two full seasons of
  history exist, otherwise a straight-line trend). Unusual periods are flagged with z-score and IQR.
  The model only narrates those computed numbers. A question that needs several results is split into
  up to three parts, each with its own query and chart.
- The prompt box completes from earlier questions and from table and column names. Tab accepts.
- **SQL-only mode:** with no model, or when the question starts with `SELECT`/`WITH`, the text runs as SQL.
- **SQL guard:** every query is parsed and must be a single read-only `SELECT` over workspace tables.
  Queries run in a rolled-back transaction with a 30-second timeout and a 5,000-row cap.
- **Show SQL** lists every attempt with its status, timing and the tables it read. **Re-run** refreshes
  the result against current data. Use thumbs up or down to rate an answer.
- Chats are saved per workspace, searchable from the sidebar, and exportable to Markdown.

## Demo data

Click **Load sample data** in onboarding or on an empty **Connections** page. LocalBoard generates a
deterministic e-commerce store (customers, products, orders, order_items, shipments, refunds; regions West,
East, Central, South; 18 full months ending last month) as CSV inside the workspace, connects it and adds the
E-commerce starter. In the last month West gross sales are flat, but West shipments were delayed far more often
and those orders were refunded far more often, so West net revenue is down. Ask
“Why is the West region down last month?”

## Agents

Every workspace starts with one analyst. Add more from **Agents** (for example Finance or Ops). Each has a description, optional model overrides, and an optional set of tables; leave the tables empty to use everything connected.

- **Rules** are definitions and per-table gotchas. Every save keeps the previous wording, with a line diff and restore. Saves and restores apply immediately.
- **Examples** are question → SQL pairs, including **Save as example** on an answer. The closest ones are passed to the model.
- **Memory** is a standing instruction. Say something like "from now on, exclude test accounts" and the analyst asks before keeping it. Active memories apply to later questions. Edit them on the Agents page.
- **Knowledge** is a PDF, Word, Markdown or text file. Excerpts are retrieved when a question is close to them.
- A question is routed to one analyst from names, descriptions, rules and examples. No extra chat-model call. The chosen analyst is the first step of the answer.

## Query Bench

**Query Bench** is a SQL editor with autocomplete from the workspace tables and columns. Format rewrites the statement through the SQL parser. Run, history and saved queries all go through the same read-only guard (30 seconds, 5,000 rows). Named parameters are `:name` and are bound by DuckDB. The grid sorts and filters. Export writes CSV, JSON, XLSX or Parquet. EXPLAIN shows the plan. Explain, Fix and Optimize ask the local model; Fix and Optimize only return SQL that passes the guard. Save as example stores a question and the query on an analyst. Save as tile adds the query to a dashboard.

## Dashboards

**Dashboards** builds a board from a prompt. The model proposes tiles; each SQL statement is guarded and run before the tile is kept. A tile that fails is dropped and named in a warning. Tiles are KPI, line, bar, stacked bar, area, pie, table, funnel or heatmap. Drag and resize the grid; the layout is saved. Global filters are bound parameters (`:name`), the same rule as Query Bench. Clicking a segment on one tile sets that filter on the others. Each chart has an insight sentence written from facts computed in Rust; the model only narrates those numbers, and a refresh skips the model when the facts have not changed. Refresh can run on an interval while the window is open, and keeps running after you close the window to the tray. Duplicate, folders and version restore are on the board.

## Datasites, reports and decks

Home and Chat have **Choose an output**: Answer (default), Dashboard, Datasite, PDF report or Presentation. **Datasites** is under Advanced in the sidebar.

A **datasite** is a multi-page site (overview plus sections) served at `http://127.0.0.1:17890/d/{id}` while LocalBoard is running. The model proposes pages; each chart SQL is guarded and run before the page is kept. Failed charts are dropped and named. Export writes a static HTML snapshot or a live-linked HTML file against that local server. There is no LAN bind.

A **PDF report** is narrative, charts, tables and a SQL appendix. A **presentation** is an editable outline; export writes PPTX with charts as images. Settings → General holds a workspace brand name and `#RRGGBB` colour used on datasites, reports and decks. From an answer, **Save as datasite / PDF / deck** reuses that answer’s guarded SQL and computed facts. It does not run a second unguarded query.

Every kept generate and every later refresh writes Audit and lineage the same way as dashboard tiles.

## Automations

**Automations** are a linear pipeline. Triggers: interval (60 seconds or longer), cron (five or six fields, local time), file change (existing file watcher), manual, or a loopback webhook.

- **Steps:** Run SQL, Ask Analyst, refresh a dashboard, compare to the previous run, threshold check, anomaly check (z-score / IQR, same facts as why/forecast), generate a PDF report or deck (Prompt A exporters), or email.
- Compare, threshold and anomaly compute in Rust. Alerts are English templates from those facts. The model may narrate the same facts; it does not invent numbers.
- Every SQL step goes through `sql_guard` and `engine::query::run` (30 seconds, 5,000 rows). Audit and lineage are written. Actor is `local`.
- **Draft with AI** saves `needs review`. Confirm before the job can be enabled or run.
- Run history is per step.
- **Webhook:** `POST http://127.0.0.1:17890/hooks/{id}` with `Authorization: Bearer <token>`. Token is shown once; the hash is stored. Loopback only.
- **Email:** Settings → General → Email. Off by default. Host/user/from in the workspace; password in the keychain. Labelled as leaving the device.

## Command palette

**⌘K / Ctrl-K** searches chats, dashboards, datasites, apps, automations and saved queries, and jumps to common pages.

## Audit

**Audit** lists every recorded action, newest first. Filter by kind and status (ok / error / denied), and search the question and SQL. Open a row for the timestamp, actor (`local`), kind, status, duration, rows, question, the tables that query read, full SQL and `detail_json`. The list shows a clipped statement; nothing is re-run from this page. Export writes the current filter to JSON or CSV.

Analyst questions, re-runs and connection changes were already written. Query Bench runs, each dashboard tile refresh, each datasite/report/deck kept run or refresh, and each automation run are also written.

## Lineage

Every guarded query that passes the SQL guard records the tables it read. On **Schema**, a table’s **Used by** list shows chats, saved queries, dashboard tiles, datasites, reports, decks and automations that read it, plus the last audit time. **Show SQL** and Audit detail list those tables. Removing a connection is blocked while anything still depends on its tables; the dialog names them. Lineage starts from an earlier milestone; older audit rows stay question and SQL only.

## MCP

LocalBoard is both an MCP client and a local MCP server. The server binds `127.0.0.1:17890/mcp` (HTTP JSON-RPC) and also understands stdio when you launch the app as `localboard mcp`. That stdio process proxies to the running app. Tools: `list_sources`, `get_schema`, `ask_analyst`, `run_query` (read-only, guarded), `list_dashboards`, `open_dashboard`, `get_insight`, `run_automation`. Every tool call writes Audit as `mcp.tool` with server, tool and read-only — not the payload. Write-like tool names are denied unless you allow them on an analyst. Tools that would run from a cell, file or retrieved document are refused until you confirm them.

Setup: leave LocalBoard running, then add the entry below to Claude Desktop (`claude_desktop_config.json`) or Cursor (MCP settings). Use the path of the installed app binary. If the local server requires a bearer token, copy it from Settings → Integrations and pass it to the proxy as the server instructs there; never paste it into a shared config.

**Claude Desktop** (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "localboard": {
      "command": "/path/to/localboard",
      "args": ["mcp"]
    }
  }
}
```

**Cursor** (MCP settings):

```json
{
  "mcpServers": {
    "localboard": {
      "command": "/path/to/localboard",
      "args": ["mcp"]
    }
  }
}
```

Leave LocalBoard running. The proxy talks to `http://127.0.0.1:17890/mcp`.

Add other MCP servers in Settings → Integrations (stdio command or HTTP URL). Per-analyst allow/deny is on **Agents**. HTTP MCP that is not loopback is blocked in Offline mode.

## Governance

Settings → Security.

- **Offline mode** is visible in the app. It blocks Hugging Face downloads, SMTP and remote MCP servers. Loopback (datasites, local MCP, llama-server) stays up.
- **App lock** is an optional passphrase. Unlock each launch.
- **Column masks** (Schema): redact, hash or email, applied at the query-result boundary.
- **Workspace encryption** is SQLCipher, opt-in. Existing workspaces stay unencrypted. The passphrase lives in the keychain.
- **Backup** exports/imports the workspace SQLite and DuckDB files as a zip. Secrets are not included.

## Templates

Home has starters for E-commerce / DTC, SaaS, subscription boxes, healthcare RCM, logistics / trucking, manufacturing, wholesale, construction and EdTech. Each adds an analyst, rules and five sample questions, plus a dashboard. Dashboard tiles use conventional table names (for example `orders`, `claims`, `loads`) and are checked against your connected tables when you apply: a tile whose tables or columns are missing is skipped and reported, never replaced with placeholder SQL. Apply again after connecting more data. Existing names are left alone.

## Packaging

`npm run tauri build` writes a platform installer. Keep the `localboard mcp` binary path in Claude Desktop / Cursor config pointed at the installed app.

**Signing (placeholders, off by default).**
- macOS: set `APPLE_SIGNING_IDENTITY` (or `bundle.macOS.signingIdentity`) to your Developer ID Application identity; for notarization set `APPLE_ID`, `APPLE_PASSWORD` (app-specific) and `APPLE_TEAM_ID`, or `APPLE_API_ISSUER`, `APPLE_API_KEY` and `APPLE_API_KEY_PATH`. Hardened runtime is already on in `src-tauri/tauri.conf.json`.
- Windows: set `bundle.windows.certificateThumbprint` and `timestampUrl` (or a `signCommand`) in `tauri.conf.json`, or use `TAURI_WINDOWS_SIGNTOOL_PATH` with your certificate.

**Deep links.** The `localboard://` scheme is registered: `localboard://dashboard/<id>` opens a dashboard and `localboard://chat/<id>` opens a conversation. Install the built app once so the OS registers the scheme.

## Privacy

Analysis happens entirely on this machine unless you turn on something labelled as leaving the device. The chat model and the embedding model each listen only on `127.0.0.1`, with their own per-launch API key. Datasites, reports, webhooks and MCP listen on `127.0.0.1:17890` for the life of the app. Outbound traffic, when not in Offline mode: a model download from Hugging Face (which redirects to its CDN), database connections to hosts you enter, optional SMTP, optional remote MCP HTTP, an optional DuckDB scanner-extension download on first live-table register, and building from source (llama.cpp from GitHub).

## Not in this milestone

LAN sharing, binding `0.0.0.0`, Ollama, column-level lineage, a lineage graph, hosted cloud workspace sharing, local users and roles, cloud identity providers, and an auto-updater feed hosted by LocalBoard.
