import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import Spinner from "./Spinner";

function b64ToBytes(b64: string): Uint8Array {
  const s = atob(b64);
  const a = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
  return a;
}

export default function TerminalView({
  sessionId,
  filePath,
  active,
  cwd,
}: {
  sessionId: string;
  filePath: string;
  active: boolean;
  cwd?: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [exited, setExited] = useState(false);
  const [started, setStarted] = useState(false);
  const [blocked, setBlocked] = useState<{ owner: string | null } | null>(null);
  const blockedRef = useRef(false);
  blockedRef.current = blocked !== null;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const term = new Terminal({
      fontSize: 13,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      theme: { background: "#0a0a0a", foreground: "#e6e6e6" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);

    // macOS line-editing keys: xterm doesn't forward Cmd-modified keys to the
    // PTY (and the webview may grab ⌘←/⌫ as browser nav), so map them to the
    // readline control sequences claude/shells understand. Returning false tells
    // xterm not to process the event (we sent it ourselves).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      // Shift+Enter → insert a newline, not submit. xterm.js encodes a bare CR
      // for both Enter and Shift+Enter (the Shift is only disambiguated under the
      // kitty/modifyOtherKeys protocols, which this embed doesn't negotiate), so
      // claude can't tell them apart and submits. Emit ESC+CR — the Meta/Option+
      // Enter sequence that is claude's built-in newline binding (no /terminal-setup).
      if (e.key === "Enter" && e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        invoke("terminal_input", { sessionId, data: "\x1b\r" }).catch(() => {});
        return false;
      }
      if (e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        const seq =
          e.key === "ArrowLeft"
            ? "\x01" // ⌘← → beginning of line (Ctrl-A)
            : e.key === "ArrowRight"
              ? "\x05" // ⌘→ → end of line (Ctrl-E)
              : e.key === "Backspace"
                ? "\x15" // ⌘⌫ → delete to beginning of line (Ctrl-U)
                : null;
        if (seq) {
          e.preventDefault();
          invoke("terminal_input", { sessionId, data: seq }).catch(() => {});
          return false;
        }
      }
      return true;
    });
    // NB: term.open() is deferred to the activation effect. Opening while the
    // host is hidden (display:none → 0-size) makes xterm measure the character
    // cell at 0, so fit() can never compute correct columns afterward — the
    // cause of the "narrow terminal" for a session first opened on the Manager
    // tab. We open only once the pane is actually visible.
    termRef.current = term;
    fitRef.current = fit;

    const w = getCurrentWebviewWindow();
    const unsubs: Array<() => void> = [];
    let disposed = false;

    (async () => {
      unsubs.push(
        await w.listen<{ sessionId: string; data: string }>("terminal-output", (e) => {
          if (e.payload.sessionId === sessionId) {
            term.write(b64ToBytes(e.payload.data));
            setStarted(true);
          }
        }),
        await w.listen<{ sessionId: string }>("terminal-exit", (e) => {
          if (e.payload.sessionId === sessionId) {
            term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
            setExited(true);
          }
        }),
      );
      if (disposed) unsubs.forEach((u) => u());
      // NB: terminal_open is deferred to the activation effect below, so the PTY
      // is spawned only once the host is visible and correctly sized.
    })();

    const onData = term.onData((d) => {
      if (blockedRef.current) return;
      invoke("terminal_input", { sessionId, data: d }).catch(() => {});
    });

    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      // Skip while hidden (display:none → 0-size) so we never fit to a tiny size.
      if (host.clientWidth === 0 || host.clientHeight === 0) return;
      // A blocked window must not resize the owner's PTY (pool is session-keyed).
      if (blockedRef.current) return;
      // Debounce: a window drag fires this dozens of times — resize once it
      // settles so claude gets a single clean SIGWINCH to repaint at the final size.
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        try {
          fit.fit();
          invoke("terminal_resize", { sessionId, rows: term.rows, cols: term.cols }).catch(() => {});
        } catch {
          /* ignore */
        }
      }, 120);
    });
    ro.observe(host);

    return () => {
      disposed = true;
      unsubs.forEach((u) => u());
      onData.dispose();
      ro.disconnect();
      clearTimeout(resizeTimer);
      term.dispose();
    };
  }, [sessionId, filePath]);

  const openedRef = useRef(false);
  // On first activation (pane visible): open the xterm into the now-laid-out
  // host, fit, and spawn the PTY at the correct size. On later activations just
  // refit. Always resize afterward to sync an existing PTY (e.g. after reload).
  useEffect(() => {
    if (!active) return;
    const t = termRef.current;
    const f = fitRef.current;
    const host = hostRef.current;
    if (!t || !f || !host) return;
    const raf = requestAnimationFrame(() => {
      if (!openedRef.current) {
        openedRef.current = true;
        t.open(host);
        try {
          f.fit();
        } catch {
          /* ignore */
        }
        invoke<{ status: string; owner: string | null }>("terminal_open", {
          sessionId,
          filePath,
          rows: t.rows,
          cols: t.cols,
          newCwd: cwd ?? null,
        })
          .then((res) => {
            if (res.status === "busy") setBlocked({ owner: res.owner });
          })
          .catch((err) => t.write(`\r\n\x1b[31m[failed to start: ${err}]\x1b[0m\r\n`));
      } else {
        try {
          f.fit();
        } catch {
          /* ignore */
        }
      }
      // Sync the PTY to the current fit (covers reconnecting to an existing PTY).
      invoke("terminal_resize", { sessionId, rows: t.rows, cols: t.cols }).catch(() => {});
      t.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [active, sessionId, filePath]);

  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    setStarted(false);
    setBlocked(null);
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => setExited(false))
      .catch(() => {});
  };

  return (
    <div className="relative h-full w-full" style={{ display: active ? "block" : "none" }}>
      <div ref={hostRef} className="h-full w-full" />
      {!started && !exited && !blocked && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-[var(--color-text-dim)] pointer-events-none">
          <Spinner size="md" />
          <span className="text-sm">Starting claude…</span>
        </div>
      )}
      {blocked && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 text-center text-[var(--color-text-dim)]">
          <span className="text-sm">This session is already open in another window.</span>
          {blocked.owner && (
            <button
              onClick={() => invoke("focus_window", { label: blocked.owner }).catch(() => {})}
              className="px-3 py-1 rounded bg-[var(--color-accent)] text-white text-xs"
            >
              Switch to it
            </button>
          )}
        </div>
      )}
      {exited && (
        <button
          onClick={restart}
          className="absolute top-2 right-2 px-3 py-1 rounded bg-[var(--color-accent)] text-white text-xs"
        >
          Restart
        </button>
      )}
    </div>
  );
}
