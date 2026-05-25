# ras-file-core

Core runtime types shared by generated file-service servers and clients.

This crate is transport-neutral: it defines file errors, request context,
incoming upload streams, JSON upload responses, and download response builders.
Generated Axum code adapts HTTP requests into these types.
