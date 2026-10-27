# Save agent conversation references

The optional `scripts/agent_session.py` hook saves Codex and Claude Code's
reported conversation ID when the provider emits `SessionStart`. It uses Python 3's standard
library and the `hc` on PATH. Copy the script to a stable location on each
machine where agents run; it is not installed by the desktop app.

Add this entry to the agent's existing `hooks.SessionStart` array, keeping its
other hooks. For Codex, use `~/.codex/hooks.json`; for Claude Code, use
`~/.claude/settings.json` and change the last argument to `claude`.

```json
{
  "hooks": [
    {
      "type": "command",
      "command": "python3 /absolute/path/agent_session.py codex",
      "timeout": 10
    }
  ]
}
```

On Windows, use the installed Python executable and quote paths in the command:

```json
"command": "py -3 \"C:\\path\\agent_session.py\" codex"
```

The hook runs only inside a core terminal. It uses `HC_TERMINAL_SOCKET` to reach
that terminal's own core, even if `HC_SOCKET` selects a different session.
The terminal must still be live. Subagent events are ignored; new, resumed,
cleared and forked root conversations add their references. Repeated reports
of the exact same reference are harmless. Existing sessions need another SessionStart
event before they are captured. Follow the agent's hook approval/reload process
after editing its configuration.

The tested Codex CLI `0.164.0-fork.2` emits this hook at the first turn, before
the first model request. Opening its prompt without starting a turn does not
capture an ID. Actual upstream Codex and Claude execution remain unverified.

The core stores a document named `herdr-agent-t7` for terminal `t7`, containing
`references`, a list of distinct `{agent, session_id, cwd}` records. Overlapping
hooks merge their references with version checks, so a late old hook cannot
erase a newer conversation. The list does not identify an active/latest session.
A successful hook means the core acknowledged
the write to disk; a failure returns a nonzero exit code and a short error.
The document belongs to that core and survives full stop, reboot and holder loss.
It does not contain prompts, transcripts, command arguments or environment data.
The core's existing 1 MiB document limit still applies. At that limit a new
reference fails visibly; the helper never silently evicts an older reference.

This saves the mapping for recovery; it does not resume the agent. Kept terminals
still return as shells. Use the saved ID with the agent's own resume command in
the original pane. Disabling keep, ending or deleting a terminal does not launch
anything or erase the reference document. Automatic resume and reference cleanup
remain separate client policy; a saved reference is not evidence that a session
is still eligible to run.

Hook contracts: [Codex](https://learn.chatgpt.com/docs/hooks) and
[Claude Code](https://code.claude.com/docs/en/hooks).

Run the owning tests with `python3 -B -m unittest discover -s scripts -p test_agent_session.py`.
Set `HC_AGENT_TEST_CORE` to an `hc` executable to also exercise real PTYs, full
stop, simultaneous core/holder termination and a failed persistence write. These
probes use their own temporary home and core session. No model requests are made.
