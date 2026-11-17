import assert from "node:assert/strict";
import test from "node:test";
import { VERSION_FILES } from "../release-meta.mjs";

const lockFile = VERSION_FILES.find(({ file }) => file === "Cargo.lock");
for (const [name, eol] of [["LF", "\n"], ["CRLF", "\r\n"]]) {
  test(`workspace lock versions support ${name} without changing dependencies`, () => {
    const before = [
      "version = 4",
      "",
      "[[package]]",
      'name = "codex-plus-core"',
      'version = "2.7.1"',
      "",
      "[[package]]",
      'name = "codex-plus-data"',
      'version = "2.7.1"',
      "",
      "[[package]]",
      'name = "tauri-plugin-dialog"',
      'version = "2.7.1"',
      'source = "registry+https://github.com/rust-lang/crates.io-index"',
      "",
    ].join(eol);
    assert.equal(lockFile.read(before), "2.7.1");
    const after = lockFile.bump(before, "2.7.1", "2.7.2");
    assert.equal(lockFile.read(after), "2.7.2");
    assert.equal(
      after,
      before
        .replace(`name = "codex-plus-core"${eol}version = "2.7.1"`,
          `name = "codex-plus-core"${eol}version = "2.7.2"`)
        .replace(`name = "codex-plus-data"${eol}version = "2.7.1"`,
          `name = "codex-plus-data"${eol}version = "2.7.2"`),
    );
  });
}
