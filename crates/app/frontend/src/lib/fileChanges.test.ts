import { describe, it, expect } from "vitest";
import {
  changeCounts,
  fileChangesText,
  fileHeader,
  filesLabel,
  fileView,
  moreFilesText,
} from "./fileChanges";
import type { FileChange, FileChanges, Hunk } from "../types";

const hunk = (new_start: number, lines: string[]): Hunk => ({
  old_start: new_start,
  old_lines: lines.filter((l) => !l.startsWith("+")).length,
  new_start,
  new_lines: lines.filter((l) => !l.startsWith("-")).length,
  lines,
});

const changelog: FileChange = {
  path: "/repo/CHANGELOG.md",
  created: false,
  hunks: [hunk(8, [" **Added**", "+- new entry", " - old entry"]), hunk(40, [" a", "-b", "+B"])],
};
const created: FileChange = {
  path: "/repo/new.rs",
  created: true,
  hunks: [hunk(1, ["+fn main() {", "+}"])],
};

describe("changeCounts", () => {
  it("counts every changed file, including ones left out, and the shown lines", () => {
    const changes: FileChanges = { files: [changelog, created], more: 3 };
    expect(changeCounts(changes)).toEqual({ files: 5, added: 4, removed: 1 });
  });
});

describe("filesLabel", () => {
  it("pluralizes", () => {
    expect(filesLabel(1)).toBe("1 file");
    expect(filesLabel(2)).toBe("2 files");
  });
});

describe("fileView", () => {
  it("says whether the file was created or updated, with its counts", () => {
    expect(fileView(changelog)).toMatchObject({ verb: "Updated", path: "/repo/CHANGELOG.md", added: 2, removed: 1 });
    expect(fileView(created)).toMatchObject({ verb: "Created", added: 2, removed: 0 });
  });

  it("maps line prefixes to diff lines indexed into each side's text", () => {
    const [, second] = fileView(changelog).hunks;
    expect(second.old).toBe("a\nb");
    expect(second.new).toBe("a\nB");
    expect(second.lines).toEqual([
      { kind: "ctx", text: "a", side: "new", index: 0 },
      { kind: "del", text: "b", side: "old", index: 1 },
      { kind: "add", text: "B", side: "new", index: 1 },
    ]);
  });

  it("marks where a hunk starts, except at the top of the file", () => {
    const [first, second] = fileView(changelog).hunks;
    expect(first.gap).toBe("⋯ line 8");
    expect(second.gap).toBe("⋯ line 40");
    expect(fileView(created).hunks[0].gap).toBeNull();
  });

  it("drops diff's no-newline marker", () => {
    const file: FileChange = { path: "/a", created: false, hunks: [hunk(3, ["-x", "\\ No newline at end of file", "+y"])] };
    expect(fileView(file).hunks[0].lines.map((l) => l.text)).toEqual(["x", "y"]);
  });
});

describe("fileHeader", () => {
  it("names the change and counts its lines", () => {
    expect(fileHeader(fileView(changelog))).toBe("Updated /repo/CHANGELOG.md +2 −1");
    expect(fileHeader(fileView(created))).toBe("Created /repo/new.rs +2");
  });
});

describe("moreFilesText", () => {
  it("counts files left out", () => {
    expect(moreFilesText({ files: [created], more: 0 })).toBeNull();
    expect(moreFilesText({ files: [created], more: 1 })).toBe("…and 1 more file");
    expect(moreFilesText({ files: [created], more: 2 })).toBe("…and 2 more files");
  });
});

describe("fileChangesText", () => {
  it("is every drawn line in order: header, gap, diff lines, then files left out", () => {
    expect(fileChangesText({ files: [changelog, created], more: 2 })).toBe(
      [
        "Updated /repo/CHANGELOG.md +2 −1",
        "⋯ line 8",
        "  **Added**",
        "+ - new entry",
        "  - old entry",
        "⋯ line 40",
        "  a",
        "- b",
        "+ B",
        "Created /repo/new.rs +2",
        "+ fn main() {",
        "+ }",
        "…and 2 more files",
      ].join("\n"),
    );
  });
});
