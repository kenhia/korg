#!/usr/bin/env python3
"""Live wire-protocol smoke test for the korg HTTP MCP endpoint.

Speaks Streamable-HTTP JSON-RPC to a running korg-api `/mcp` route exactly as a
real MCP client (e.g. Claude/Copilot) would: initialize -> tools/list ->
tools/call. Validates the server handshakes, advertises its tools, and returns
real data from the live korg database.

Exit 0 = all checks passed; non-zero = a check failed (objective gate).

Env:
  KORG_MCP_URL  optional, the /mcp endpoint (default http://localhost:8090/mcp).
"""
import json
import os
import sys
import urllib.request

URL = os.environ.get("KORG_MCP_URL", "http://localhost:8090/mcp")
HEADERS = {
    "content-type": "application/json",
    "accept": "application/json, text/event-stream",
    "mcp-protocol-version": "2025-06-18",
}


def rpc(rid, method, params=None):
    body = json.dumps(
        {"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}
    ).encode()
    req = urllib.request.Request(URL, data=body, headers=HEADERS, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def check(label, cond):
    status = "ok" if cond else "FAIL"
    print(f"[{status}] {label}")
    return cond


def main():
    ok = True

    init = rpc(1, "initialize", {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "mcp-live-check", "version": "0"},
    })
    ok &= check(
        f"initialize -> serverInfo.name == korg-mcp ({init.get('result', {}).get('serverInfo', {}).get('name')})",
        init.get("result", {}).get("serverInfo", {}).get("name") == "korg-mcp",
    )

    tools = rpc(2, "tools/list")
    n_tools = len(tools.get("result", {}).get("tools", []))
    ok &= check(f"tools/list -> {n_tools} tools (>=16)", n_tools >= 16)

    wi = rpc(3, "tools/call", {"name": "list_work_items", "arguments": {}})
    text = wi.get("result", {}).get("content", [{}])[0].get("text", "[]")
    n_wi = len(json.loads(text))
    ok &= check(f"list_work_items -> {n_wi} work items (>=1)", n_wi >= 1)

    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # noqa: BLE001
        print(f"[FAIL] {type(e).__name__}: {e}")
        sys.exit(1)
