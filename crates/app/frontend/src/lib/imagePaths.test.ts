import { describe, it, expect, vi } from "vitest";
import { codeImagePath, codeImagePaths, makeImageLoader, resolveImageSrc, savedImagePaths } from "./imagePaths";
import type { RenderedImage, RenderedTurn } from "../types";

describe("resolveImageSrc", () => {
  it("keeps absolute and ~ paths", () => {
    expect(resolveImageSrc("/abs/a.png", null)).toBe("/abs/a.png");
    expect(resolveImageSrc("~/a.png", null)).toBe("~/a.png");
  });
  it("strips file:// and percent-decodes", () => {
    expect(resolveImageSrc("file:///tmp/a%20b.png", null)).toBe("/tmp/a b.png");
    expect(resolveImageSrc("/tmp/a%20b.png", null)).toBe("/tmp/a b.png");
  });
  it("joins relative paths to the session cwd", () => {
    expect(resolveImageSrc("shots/a.png", "/repo")).toBe("/repo/shots/a.png");
    expect(resolveImageSrc("./a.png", "/repo/")).toBe("/repo/a.png");
    expect(resolveImageSrc("shots/a.png", null)).toBeNull();
  });
  it("leaves remote, data and non-image sources alone", () => {
    expect(resolveImageSrc("https://x.com/a.png", "/repo")).toBeNull();
    expect(resolveImageSrc("http://x.com/a.png", "/repo")).toBeNull();
    expect(resolveImageSrc("data:image/png;base64,AAA", "/repo")).toBeNull();
    expect(resolveImageSrc("//cdn.x.com/a.png", "/repo")).toBeNull();
    expect(resolveImageSrc("/tmp/notes.txt", "/repo")).toBeNull();
    expect(resolveImageSrc(undefined, "/repo")).toBeNull();
  });
});

describe("codeImagePath", () => {
  it("accepts absolute and ~ image paths only", () => {
    expect(codeImagePath("/tmp/cover.jpg")).toBe("/tmp/cover.jpg");
    expect(codeImagePath(" ~/.config/x/shot.PNG ")).toBe("~/.config/x/shot.PNG");
    expect(codeImagePath("src/a.png")).toBeNull();
    expect(codeImagePath("//host/a.png")).toBeNull();
    expect(codeImagePath("/tmp/a.rs")).toBeNull();
    expect(codeImagePath("/a.png\n/b.png")).toBeNull();
  });
});

describe("codeImagePaths", () => {
  const text = (value: string) => ({ type: "text", value });
  const el = (tagName: string, children: unknown[]) => ({ type: "element", tagName, children });
  it("collects inline code image paths, including inside emphasis, deduped", () => {
    const p = el("p", [
      text("saved "),
      el("code", [text("/tmp/a.png")]),
      el("strong", [el("code", [text("/tmp/b.jpg")])]),
      el("code", [text("/tmp/a.png")]),
      el("code", [text("cargo test")]),
    ]);
    expect(codeImagePaths(p)).toEqual(["/tmp/a.png", "/tmp/b.jpg"]);
  });
  it("leaves nested blocks to their own renderer", () => {
    const li = el("li", [el("code", [text("/tmp/a.png")]), el("ul", [el("li", [el("code", [text("/tmp/b.png")])])])]);
    expect(codeImagePaths(li)).toEqual(["/tmp/a.png"]);
    expect(codeImagePaths(el("li", [el("p", [el("code", [text("/tmp/c.png")])])]))).toEqual([]);
    expect(codeImagePaths(undefined)).toEqual([]);
  });
});

const img = (data: string): RenderedImage => ({ media_type: "image/png", data });
const toolTurn = (text: string, images?: RenderedImage[]): RenderedTurn => ({
  type: "assistant",
  timestamp: null,
  model: null,
  effort: null,
  uuid: "u",
  edit_index: 0,
  fork_index: 0,
  edit_uuid: "u",
  fork_uuid: "u",
  uuids: ["u"],
  editable: true,
  blocks: [
    { type: "text", text: "hi" },
    { type: "tool_call", id: "t", name: "screenshot", input_summary: "", input_full: {}, result: { text, is_error: false, images } },
    { type: "tool_call", id: "t2", name: "x", input_summary: "", input_full: {}, result: null },
  ],
});

describe("savedImagePaths", () => {
  it("pairs each 'Screenshot saved to' line with the image at the same index", () => {
    const m = savedImagePaths([
      toolTurn("Screenshot saved to: /var/f/s-1.png\nScreenshot saved to: /var/f/s-2.jpg\nInclude the path.", [img("A"), img("B")]),
      { type: "system", timestamp: null, content: "x" },
    ]);
    expect(m.get("/var/f/s-1.png")?.data).toBe("A");
    expect(m.get("/var/f/s-2.jpg")?.data).toBe("B");
  });
  it("maps nothing when the counts disagree", () => {
    expect(savedImagePaths([toolTurn("Screenshot saved to: /var/f/s.png", [])]).size).toBe(0);
    expect(savedImagePaths([toolTurn("Screenshot saved to: /var/f/s.png", [img("A"), img("B")])]).size).toBe(0);
  });
});

describe("makeImageLoader", () => {
  it("reads each path once", async () => {
    const invoke = vi.fn(async () => img("X"));
    const load = makeImageLoader(invoke);
    const [a, b] = await Promise.all([load("/a.png"), load("/a.png")]);
    expect(a).toBe(b);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("read_image_file", { path: "/a.png" });
  });
});
