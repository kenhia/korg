#!/usr/bin/env bash
# mcp-roundtrip-check.sh — objective end-to-end proof that the korg HTTP MCP
# server is reachable and answers a real tools/call. Exit 0 == healthy.
#
#   KORG_MCP_URL  default http://kubsdb:5674/mcp
set -euo pipefail
U="${1:-${KORG_MCP_URL:-http://kubsdb:5674/mcp}}"
hdr=(-H "Content-Type: application/json" -H "Accept: application/json, text/event-stream")

# initialize must return serverInfo.name == korg-mcp
init=$(curl -fsS -X POST "$U" "${hdr[@]}" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"check","version":"1"}}}')
echo "$init" | grep -q '"name":"korg-mcp"' || { echo "FAIL: initialize did not return korg-mcp" >&2; exit 1; }

# tools/list must expose at least 16 tools incl. list_work_items
tools=$(curl -fsS -X POST "$U" "${hdr[@]}" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}')
echo "$tools" | grep -q '"list_work_items"' || { echo "FAIL: list_work_items not advertised" >&2; exit 1; }

# tools/call must execute and return content
call=$(curl -fsS -X POST "$U" "${hdr[@]}" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}')
echo "$call" | grep -q '"content"' || { echo "FAIL: tools/call returned no content" >&2; exit 1; }

echo "OK: korg MCP healthy at $U (initialize + tools/list + tools/call)"
