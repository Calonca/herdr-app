import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    if (!process.env.HC_TERMINAL_ID || !process.env.HC_TERMINAL_SOCKET) return;
    const session_file = ctx.sessionManager.getSessionFile();
    if (!session_file) return; // --no-session deliberately has no resumable file.
    const reference = {
      session_id: ctx.sessionManager.getSessionId(),
      session_file,
      cwd: ctx.sessionManager.getCwd(),
    };
    const helper = fileURLToPath(new URL("./agent_session.py", import.meta.url));
    await new Promise<void>((resolve, reject) => {
      const child = spawn(process.platform === "win32" ? "python" : "python3", [helper, "pi"], {
        stdio: ["pipe", "ignore", "pipe"],
        windowsHide: true,
      });
      let error = "";
      child.stderr.on("data", (bytes) => { error += bytes.toString("utf8"); });
      child.on("error", reject);
      child.stdin.on("error", reject);
      child.on("close", (code) => {
        if (code === 0) resolve();
        else reject(new Error(error.trim() || "Herdr could not save this Pi session."));
      });
      child.stdin.end(JSON.stringify(reference), "utf8");
    });
  });
}
