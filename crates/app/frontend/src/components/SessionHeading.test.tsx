import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SessionHeading } from "./SessionHeading";

function spanFor(html: string, text: string): string {
  const m = html.match(new RegExp(`<span[^>]*>${text}</span>`));
  if (!m) throw new Error(`no span for ${text} in ${html}`);
  return m[0];
}

describe("SessionHeading", () => {
  it("keeps the project on one line, capped, with the full name on hover", () => {
    const span = spanFor(
      renderToStaticMarkup(<SessionHeading project="status-demo" title="t" />),
      "status-demo",
    );
    expect(span).toMatch(/class="[^"]*\bshrink-0\b/);
    expect(span).toMatch(/\btruncate\b/);
    expect(span).toMatch(/\bmax-w-\[/);
    expect(span).toContain('title="status-demo"');
  });

  it("truncates the title and lets it shrink", () => {
    const span = spanFor(renderToStaticMarkup(<SessionHeading project="p" title="A long title" />), "A long title");
    expect(span).toMatch(/\bmin-w-0\b/);
    expect(span).toMatch(/\btruncate\b/);
    expect(span).toContain('title="A long title"');
  });

  it("renders a subtitle and omits missing parts", () => {
    const html = renderToStaticMarkup(<SessionHeading title="Alias" subtitle="Raw" />);
    expect(spanFor(html, "Raw")).toMatch(/\btruncate\b/);
    expect(html).not.toContain("accent");
    expect(renderToStaticMarkup(<SessionHeading project="p" />)).not.toContain("min-w-0");
  });
});
