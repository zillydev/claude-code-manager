import type { RenderedTurn } from "../types";

export interface SessionLink {
  url: string;
  /** Hostname, for the row's bold lead. */
  domain: string;
  /** Head uuid of the turn the link first appeared in ("" for command turns). */
  uuid: string;
  role: "user" | "assistant" | "command";
}

// Bare URLs and the target of markdown links. Terminators cover whitespace,
// closing brackets and quotes; trailing sentence punctuation is stripped after.
const URL_RE = /https?:\/\/[^\s<>()"'\]]+/g;

function cleaned(raw: string): string | null {
  const url = raw.replace(/[.,;:!?]+$/, "");
  try {
    return new URL(url).href.length > 0 ? url : null;
  } catch {
    return null;
  }
}

/**
 * Every URL that appeared in the conversation's own text — user messages,
 * assistant text blocks, and slash-command echoes — deduped by URL, first
 * occurrence wins. Tool results are deliberately not scanned: a fetched page
 * dumps hundreds of URLs, and the links that matter get echoed in text.
 */
export function extractLinks(turns: RenderedTurn[]): SessionLink[] {
  const seen = new Set<string>();
  const out: SessionLink[] = [];
  const scan = (text: string, uuid: string, role: SessionLink["role"]) => {
    for (const m of text.matchAll(URL_RE)) {
      const url = cleaned(m[0]);
      if (!url || seen.has(url)) continue;
      seen.add(url);
      let domain = "";
      try {
        domain = new URL(url).hostname;
      } catch {
        continue;
      }
      out.push({ url, domain, uuid, role });
    }
  };
  for (const t of turns) {
    if (t.type === "user") scan(t.text, t.uuid, "user");
    else if (t.type === "command") scan(t.command, "", "command");
    else if (t.type === "assistant") {
      for (const b of t.blocks) {
        if (b.type === "text") scan(b.text, t.uuid, "assistant");
      }
    }
  }
  return out;
}
