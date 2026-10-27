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


@unittest.skipUnless(os.environ.get("HC_AGENT_TEST_CORE"), "set HC_AGENT_TEST_CORE to an hc executable for real-PTY proof")
class DurableSessionTest(unittest.TestCase):
    def test_a_provider_hook_is_durable_through_stop_and_total_holder_loss_and_reports_write_failure(self):
        hc = os.environ["HC_AGENT_TEST_CORE"]
        with tempfile.TemporaryDirectory(prefix="hc-as-", dir=None if os.name == "nt" else "/var/tmp") as scratch:
            root = Path(scratch)
            env = dict(os.environ, HC_SESSION="test", XDG_STATE_HOME=str(root / "state"),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_RUNTIME_DIR=str(root / "run"))
            for key in ("HC_SOCKET", "HC_TERMINAL_ID", "HC_TERMINAL_SOCKET", "CODEX_THREAD_ID"):
                env.pop(key, None)
            helper = Path(agent_session.__file__).resolve()
            state = root / "state/herdr-core/test"

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
                event = {"hook_event_name": "SessionStart", "session_id": "codex-conversation-1", "cwd": scratch,
                         "source": "startup", "prompt": "never persist this prompt"}
                code = (
                    "import json,os,sys,time; "
                    f"sys.path.insert(0,{str(helper.parent)!r}); import agent_session; "
                    f"event={event!r}; "
                    f"agent_session.save(agent_session.reference('codex',event,os.environ),os.environ['HC_TERMINAL_ID'],os.environ,hc={hc!r}); "
                    f"open({str(root / 'socket')!r},'w').write(os.environ['HC_TERMINAL_SOCKET']); "
                    "print('reference saved',flush=True); time.sleep(60)"
                )
                terminal = cli("run", "--bg", "--keep", sys.executable, "-c", code)["address"]
                until(lambda: (root / "socket").exists())
                expected = {"agent": "codex", "session_id": event["session_id"], "cwd": scratch}
                self.assertEqual(document()["value"], expected)
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
                self.assertEqual(document()["value"], expected)
                before = cli("get", terminal)["pid"]
                status = cli("server", "status")
                for pid in (status["pid"], status["holder"]["pid"]):
                    os.kill(pid, signal.SIGTERM if os.name == "nt" else signal.SIGKILL)
                until(lambda: cli("server", "start")["pid"] != status["pid"])
                until(lambda: cli("get", terminal)["pid"] not in (None, before))
                self.assertEqual(document()["value"], expected)
                (state / "docs.tmp").mkdir()
                with self.assertRaises(agent_session.RpcError):
                    agent_session.save(dict(expected, session_id="claude-conversation-2", agent="claude"),
                                       terminal, hook_env, hc=hc)
                self.assertEqual(document()["value"], expected, "a failed write keeps the acknowledged reference")
                (state / "docs.tmp").rmdir()
            finally:
                subprocess.run([hc, "server", "stop"], env=env, capture_output=True, timeout=15)


if __name__ == "__main__":
    unittest.main()
