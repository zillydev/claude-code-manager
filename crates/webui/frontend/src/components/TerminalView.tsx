import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

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
}: {
  sessionId: string;
  filePath: string;
  active: boolean;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [exited, setExited] = useState(false);

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
    term.open(host);
    try {
      fit.fit();
    } catch {
      /* host may be hidden (0-size) on first mount; refit on activate */
    }
    termRef.current = term;
    fitRef.current = fit;

    const w = getCurrentWebviewWindow();
    const unsubs: Array<() => void> = [];
    let disposed = false;

    (async () => {
      unsubs.push(
        await w.listen<{ sessionId: string; data: string }>("terminal-output", (e) => {
          if (e.payload.sessionId === sessionId) term.write(b64ToBytes(e.payload.data));
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
      invoke("terminal_input", { sessionId, data: d }).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      // Skip while hidden (display:none → 0-size) so we never fit to a tiny size.
      if (host.clientWidth === 0 || host.clientHeight === 0) return;
      try {
        fit.fit();
        invoke("terminal_resize", { sessionId, rows: term.rows, cols: term.cols }).catch(() => {});
      } catch {
        /* ignore */
      }
    });
    ro.observe(host);

    return () => {
      disposed = true;
      unsubs.forEach((u) => u());
      onData.dispose();
      ro.disconnect();
      term.dispose();
    };
  }, [sessionId, filePath]);

  const openedRef = useRef(false);
  // Spawn the PTY only once the terminal is visible and fitted — a PTY spawned
  // while hidden gets a ~0-col size and claude renders badly wrapped. On later
  // activations just refit + resize.
  useEffect(() => {
    if (!active) return;
    const t = termRef.current;
    const f = fitRef.current;
    if (!t || !f) return;
    const raf = requestAnimationFrame(() => {
      try {
        f.fit();
      } catch {
        /* ignore */
      }
      if (!openedRef.current) {
        openedRef.current = true;
        invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols }).catch((err) =>
          t.write(`\r\n\x1b[31m[failed to start: ${err}]\x1b[0m\r\n`),
        );
      } else {
        invoke("terminal_resize", { sessionId, rows: t.rows, cols: t.cols }).catch(() => {});
      }
      t.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [active, sessionId, filePath]);

  const restart = () => {
    const t = termRef.current;
    if (!t) return;
    t.clear();
    invoke("terminal_open", { sessionId, filePath, rows: t.rows, cols: t.cols })
      .then(() => setExited(false))
      .catch(() => {});
  };

  return (
    <div className="relative h-full w-full" style={{ display: active ? "block" : "none" }}>
      <div ref={hostRef} className="h-full w-full" />
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
