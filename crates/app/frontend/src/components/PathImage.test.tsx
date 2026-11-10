import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import { ImagePathContext, MD_IMAGE_COMPONENTS, mdUrlTransform } from "./PathImage";
import type { ImageLoader } from "../lib/imagePaths";
import type { RenderedImage } from "../types";

const shot: RenderedImage = { media_type: "image/png", data: "QUJD" };

function render(md: string, saved = new Map<string, RenderedImage>(), cwd: string | null = "/repo", load: ImageLoader | null = null) {
  return renderToStaticMarkup(
    <ImagePathContext.Provider value={{ saved, cwd, load }}>
      <Markdown components={MD_IMAGE_COMPONENTS} urlTransform={mdUrlTransform}>
        {md}
      </Markdown>
    </ImagePathContext.Provider>,
  );
}

describe("markdown image of a local path", () => {
  it("shows a session-held screenshot as a data URL with its path", () => {
    const html = render("see ![Default view](/var/f/s-1.png) here", new Map([["/var/f/s-1.png", shot]]));
    expect(html).toContain('src="data:image/png;base64,QUJD"');
    expect(html).toContain("/var/f/s-1.png");
    expect(html).toContain("Default view");
  });

  it("wraps in a span so the paragraph holds no div", () => {
    const html = render("text ![a](/x.png) more", new Map([["/x.png", shot]]));
    expect(html).toMatch(/^<p>text <span/);
    expect(html.slice(0, html.indexOf("</p>"))).not.toContain("<div");
  });

  it("never emits a bare <img src=path> for a local file", () => {
    const html = render("![a](/tmp/gone.png)");
    expect(html).not.toContain('src="/tmp/gone.png"');
    expect(html).not.toContain("<img");
    expect(html).toContain("/tmp/gone.png");
    expect(html).toContain('data-label="image not found ·"');
  });

  it("keeps the same text while loading and once missing, so find offsets hold", () => {
    const text = (html: string) => html.replace(/<[^>]*>/g, "");
    const md = "![shot](/tmp/gone.png) then term";
    const loading = render(md, undefined, "/repo", () => new Promise(() => {}));
    const missing = render(md);
    expect(missing).toContain("image not found");
    expect(text(loading)).toBe(text(missing));
  });

  it("reads a file:// image source as its path", () => {
    expect(render("![a](file:///var/f/s-1.png)", new Map([["/var/f/s-1.png", shot]]))).toContain("data:image/png;base64,QUJD");
    expect(render("[x](file:///etc/passwd)")).toContain('href=""');
  });

  it("keeps remote images as plain img", () => {
    expect(render("![a](https://x.com/a.png)")).toContain('src="https://x.com/a.png"');
  });
});

describe("backticked image paths", () => {
  it("puts a thumbnail under the paragraph when the session holds the image", () => {
    const html = render("Saved to `/var/f/s-1.png` ok", new Map([["/var/f/s-1.png", shot]]));
    expect(html).toContain("<code>/var/f/s-1.png</code>");
    expect(html).toMatch(/<\/p><span[^>]*><img[^>]*src="data:image\/png;base64,QUJD"/);
  });

  it("adds nothing for a path it cannot resolve", () => {
    expect(render("The logo is `/logo.png`.")).toBe("<p>The logo is <code>/logo.png</code>.</p>");
  });

  it("works in tight list items", () => {
    const html = render("- shot: `/var/f/s-1.png`", new Map([["/var/f/s-1.png", shot]]));
    expect(html).toMatch(/<li>shot: <code>\/var\/f\/s-1.png<\/code><span[^>]*><img/);
  });
});
