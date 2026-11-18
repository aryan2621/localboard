# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Delivered as a Tauri desktop app (macOS primary) rendering a web UI; the design language is the app's own, not a native OS one.

## Users
Non-engineer analysts and business owners who point the app at their own local files and databases and ask questions in plain English. They want answers, dashboards and reports without writing SQL. Power users can also work in the Query Bench.

## Product Purpose
Lets a person turn local CSV/JSON files and connected databases into trustworthy answers, dashboards and reports on their own machine. Success is a correct, checkable answer to a plain-English question with the data never leaving the machine by default.

## Positioning
Local by default: a local LLM (Ollama / llama.cpp) plus DuckDB, guarded read-only SQL and column masking, so answers are computed on the user's machine. Cloud models and remote connectors are available only as an explicit opt-in.

## Operating Context
Desktop workbench: a data pane of tables, a home composer for asking, an answer view with steps and an inspector, a Query Bench, Connections, Models settings. Data comes from files in the workspace and from connectors.

## Capabilities and Constraints
- Ask, dashboard and report outputs; per-task model routing; per-model settings (temperature, top-p, context size for Ollama, timeout).
- SQL is read-only and guarded; masked columns must never leak through answers or exports.
- Undecided: how far cloud opt-in is surfaced in the UI.

## Evidence on Hand
Sample data at `~/Desktop/localboard-sample-data/` (`sales.csv`, `customers.json`). No testimonials, customers or benchmarks exist; do not invent any.

## Product Principles
- Answers are checkable: show the steps and the SQL behind a number.
- Private by default; anything that leaves the machine is an explicit user choice.
- Plain language first, SQL available underneath.
- Quiet, dense tool UI: the data is the focus.

## Accessibility & Inclusion
WCAG AA: visible focus, keyboard path for every action, 4.5:1 contrast, never colour alone.
