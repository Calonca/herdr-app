"""Run with python -m unittest discover -s scripts -p test_agent_session.py."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

import agent_session


class AgentSessionTest(unittest.TestCase):
    def test_only_the_root_session_reference_is_captured_without_prompt_data(self):
        event = {
            "hook_event_name": "SessionStart", "session_id": "conversation-1",
            "cwd": "C:/work", "source": "startup", "transcript_path": "C:/private/log.jsonl",
            "prompt": "private prompt", "unknown": "ignored",
        }
        for agent in ("codex", "claude"):
            saved = agent_session.reference(agent, event, {})
            self.assertEqual(saved["session_id"], "conversation-1")
            self.assertEqual(set(saved), {"agent", "session_id", "cwd"})
            for source in ("resume", "clear", "compact", "fork"):
                changed = agent_session.reference(agent, dict(event, source=source, session_id="conversation-2"), {})
                self.assertEqual(changed["session_id"], "conversation-2")
            self.assertIsNone(agent_session.reference(agent, dict(event, agent_id="child"), {}))
            self.assertIsNone(agent_session.reference(agent, dict(event, hook_event_name="SubagentStart"), {}))
        self.assertIsNone(agent_session.reference("codex", event, {"CODEX_THREAD_ID": "parent-conversation"}))
        self.assertIsNotNone(agent_session.reference("codex", event, {"CODEX_THREAD_ID": "conversation-1"}))

    def test_an_invalid_reference_fails_instead_of_overwriting_the_saved_identity(self):
        event = {"hook_event_name": "SessionStart", "session_id": "conversation", "cwd": "/work"}
        for field in ("session_id", "cwd"):
            for value in (None, "", 1, []):
                with self.assertRaises(ValueError):
                    agent_session.reference("codex", dict(event, **{field: value}), {})

    def test_a_wire_conflict_rereads_and_keeps_both_overlapping_sessions(self):
        server = '''
import json, sys
version = 1
newer = __NEWER__
for line in sys.stdin:
    request = json.loads(line)
    method, params = request["method"], request["params"]
    reply = {"jsonrpc": "2.0", "id": request["id"]}
    if method == "terminal.get":
        reply["result"] = {"address": "t1", "state": "live"}
    elif method == "doc.get":
        reply["result"] = {"version": version, "value": {"references": [newer]} if version == 2 and newer else None}
    elif method == "doc.set" and version == 1:
        version = 2
        reply["error"] = {"code": -32000, "message": "changed meanwhile", "data": {"kind": "conflict", "hint": "read and retry"}}
    elif method == "doc.set" and params["version"] == 2:
        open("saved", "w").write(json.dumps(params["value"]))
        reply["result"] = {"version": 3}
    else:
        raise AssertionError("the stale document version was retried without a read")
    print(json.dumps(reply), flush=True)
'''
        for newer in (None, {"agent": "claude", "session_id": "after-clear", "cwd": "/work"}):
            with self.subTest(newer=newer), tempfile.TemporaryDirectory(prefix="hc-cas-") as scratch:
                root = Path(scratch)
                (root / "connect").write_text(server.replace("__NEWER__", repr(newer)), encoding="utf-8")
                previous = Path.cwd()
                try:
                    os.chdir(root)
                    value = {"agent": "claude", "session_id": "before-clear", "cwd": scratch}
                    agent_session.save(value, "t1", dict(os.environ, HC_TERMINAL_SOCKET="test-only"), hc=sys.executable)
                    self.assertEqual(json.loads((root / "saved").read_text())["references"],
                                     [value] if newer is None else [newer, value],
                                     "a late hook must add its reference without erasing another session")
                finally:
                    os.chdir(previous)


@unittest.skipUnless(os.environ.get("HC_AGENT_TEST_CORE"), "set HC_AGENT_TEST_CORE to an hc executable for real-PTY proof")
class DurableSessionTest(unittest.TestCase):
    def test_a_provider_hook_is_durable_through_stop_and_total_holder_loss_and_reports_write_failure(self):
        hc = os.environ["HC_AGENT_TEST_CORE"]
        with tempfile.TemporaryDirectory(prefix="hc-as-", dir=None if os.name == "nt" else "/var/tmp") as scratch:
            root = Path(scratch)
            session = root.name
            env = dict(os.environ, HC_SESSION=session, XDG_STATE_HOME=str(root / "state"),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_RUNTIME_DIR=str(root / "run"))
            for key in ("HC_SOCKET", "HC_TERMINAL_ID", "HC_TERMINAL_SOCKET", "CODEX_THREAD_ID"):
                env.pop(key, None)
            helper = Path(agent_session.__file__).resolve()
            state = root / "state/herdr-core" / session

            def cli(*args):
                out = subprocess.run([hc, "--json", *args], env=env, capture_output=True, text=True, timeout=15)
                self.assertEqual(out.returncode, 0, out.stderr)
                return json.loads(out.stdout)

            def until(check):
                deadline = time.monotonic() + 15
                while not check():
                    self.assertLess(time.monotonic(), deadline, "session recovery timed out")
                    time.sleep(0.05)

            def document():
                return json.loads((state / "docs.json").read_text(encoding="utf-8"))["herdr-agent-" + terminal]

            try:
                cli("server", "start")
                cwd = root / "caf\u00e9"
                cwd.mkdir()
                event = {"hook_event_name": "SessionStart", "session_id": "codex-conversation-1", "cwd": str(cwd),
                         "source": "startup", "prompt": "never persist this prompt"}
                code = (
                    "import json,os,subprocess,sys,time; "
                    f"event={event!r}; "
                    f"subprocess.run([sys.executable,'-X','utf8=0','-B',{str(helper)!r},'codex'], "
                    "input=json.dumps(event,ensure_ascii=False).encode('utf-8'), "
                    f"env=dict(os.environ,PYTHONIOENCODING='cp1252',PATH={str(Path(hc).parent)!r}+os.pathsep+os.environ['PATH']),check=True); "
                    f"open({str(root / 'socket')!r},'w').write(os.environ['HC_TERMINAL_SOCKET']); "
                    "print('reference saved',flush=True); time.sleep(60)"
                )
                terminal = cli("run", "--bg", "--keep", "--cwd", str(cwd), sys.executable, "-B", "-c", code)["address"]
                until(lambda: (root / "socket").exists())
                expected = {"agent": "codex", "session_id": event["session_id"], "cwd": str(cwd)}
                self.assertEqual(document()["value"], {"references": [expected]})
                self.assertNotIn("never persist this prompt", (state / "docs.json").read_text())
                hook_env = dict(env, HC_TERMINAL_ID=terminal,
                                HC_TERMINAL_SOCKET=(root / "socket").read_text())
                # A wrong inherited HC_SOCKET must never redirect the reference to another core.
                hook_env["HC_SOCKET"] = "not-the-terminal-core"
                agent_session.save(expected, terminal, hook_env, hc=hc)
                self.assertEqual(document()["version"], 1, "a repeated report is idempotent")
                cli("server", "stop")
                cli("server", "start")
                until(lambda: cli("get", terminal)["pid"] is not None)
                self.assertEqual(document()["value"], {"references": [expected]})
                before = cli("get", terminal)["pid"]
                status = cli("server", "status")
                for pid in (status["pid"], status["holder"]["pid"]):
                    os.kill(pid, signal.SIGTERM if os.name == "nt" else signal.SIGKILL)
                until(lambda: cli("server", "start")["pid"] != status["pid"])
                until(lambda: cli("get", terminal)["pid"] not in (None, before))
                self.assertEqual(document()["value"], {"references": [expected]})
                (state / "docs.tmp").mkdir()
                with self.assertRaises(agent_session.RpcError):
                    agent_session.save(dict(expected, session_id="claude-conversation-2", agent="claude"),
                                       terminal, hook_env, hc=hc)
                self.assertEqual(document()["value"], {"references": [expected]}, "a failed write keeps the acknowledged reference")
                (state / "docs.tmp").rmdir()
            finally:
                subprocess.run([hc, "server", "stop"], env=env, capture_output=True, timeout=15)


if __name__ == "__main__":
    unittest.main()
