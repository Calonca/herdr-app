"""Save Codex/Claude SessionStart references on the terminal's own herdr-core."""

import argparse
import json
import os
import queue
import subprocess
import sys
import threading
import time


def reference(agent, event, env):
    if not isinstance(event, dict):
        raise ValueError("SessionStart must be a JSON object")
    if event.get("hook_event_name") != "SessionStart" or event.get("agent_id"):
        return None
    session = event.get("session_id")
    if agent == "codex" and env.get("CODEX_THREAD_ID") not in (None, "", session):
        return None
    for field in ("session_id", "cwd"):
        if not isinstance(event.get(field), str) or not event[field].strip():
            raise ValueError(f"SessionStart has no {field}")
    return {"agent": agent, "session_id": session, "cwd": event["cwd"]}


class RpcError(RuntimeError):
    def __init__(self, error):
        super().__init__(error.get("message", "the core rejected the reference"))
        self.kind = error.get("kind")


def save(value, terminal, env, hc="hc"):
    child_env = dict(env, HC_SOCKET=env["HC_TERMINAL_SOCKET"])
    # Hooks have a ten-second timeout; leave time to report failure and close the bridge.
    deadline = time.monotonic() + 5
    with subprocess.Popen(
        [hc, "connect"], env=child_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, text=True, encoding="utf-8",
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    ) as bridge:
        replies = queue.Queue()

        def read():
            for line in bridge.stdout:
                replies.put(line)
            replies.put(None)

        threading.Thread(target=read, daemon=True).start()
        request_id = 0

        def call(method, params):
            nonlocal request_id
            request_id += 1
            request = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
            bridge.stdin.write(json.dumps(request) + "\n")
            bridge.stdin.flush()
            try:
                line = replies.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty:
                raise RuntimeError("the core did not answer") from None
            if line is None:
                raise RuntimeError("the terminal's core is unavailable")
            reply = json.loads(line)
            if reply.get("id") != request_id:
                raise RuntimeError("unexpected reply from the terminal's core")
            if "error" in reply:
                raise RpcError(reply["error"])
            return reply["result"]

        try:
            record = call("terminal.get", {"terminal": terminal})
            if record["address"] != terminal or record["state"] != "live":
                raise RuntimeError("the reporting terminal is no longer live")
            name = "herdr-agent-" + terminal
            while time.monotonic() < deadline:
                current = call("doc.get", {"name": name})
                if current["value"] == value:
                    return
                try:
                    call("doc.set", {"name": name, "version": current["version"], "value": value})
                    return
                except RpcError as error:
                    if error.kind != "conflict":
                        raise
            raise RuntimeError("another session kept changing the saved reference")
        finally:
            bridge.stdin.close()
            try:
                bridge.wait(timeout=1)
            except subprocess.TimeoutExpired:
                bridge.terminate()
                bridge.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("agent", choices=("codex", "claude"))
    args = parser.parse_args()
    if not os.environ.get("HC_TERMINAL_ID") or not os.environ.get("HC_TERMINAL_SOCKET"):
        return 0
    try:
        event = json.load(sys.stdin.buffer)
        value = reference(args.agent, event, os.environ)
        if value is not None:
            save(value, os.environ["HC_TERMINAL_ID"], os.environ)
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        print(f"Herdr could not save this agent session: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
