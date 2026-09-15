export function patchRows(patch: string) {
  let left = 0,
    right = 0;
  const lines = patch.split("\n").slice(2);
  if (lines.at(-1) === "") lines.pop();
  return lines.map((line) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      left = Number(hunk[1]);
      right = Number(hunk[2]);
      return { type: "hunk", text: line, left: "", right: "", mark: "" };
    }
    const mark = line[0];
    if (mark === "\\")
      return {
        type: "hunk",
        text: "此处文件末尾没有换行符",
        left: "",
        right: "",
        mark: "",
      };
    return {
      type: mark === "+" ? "add" : mark === "-" ? "remove" : "context",
      text: line.slice(1),
      left: mark === "+" ? "" : String(left++),
      right: mark === "-" ? "" : String(right++),
      mark,
    };
  });
}

export type SplitRow = { left?: ReturnType<typeof patchRows>[number]; right?: ReturnType<typeof patchRows>[number]; note?: string };
export function splitPatchRows(patch: string): SplitRow[] {
  const result: SplitRow[] = [];
  let removed: ReturnType<typeof patchRows> = [], added: ReturnType<typeof patchRows> = [];
  let notes: string[] = [];
  const flush = () => {
    for (let i = 0; i < Math.max(removed.length, added.length); i++) result.push({ left: removed[i], right: added[i] });
    for (const note of new Set(notes)) result.push({ note });
    removed = []; added = []; notes = [];
  };
  for (const row of patchRows(patch)) {
    if (row.type === "remove") removed.push(row);
    else if (row.type === "add") added.push(row);
    else if (row.type === "hunk" && !row.text.startsWith("@@")) notes.push(row.text);
    else {
      flush();
      result.push(row.type === "hunk" ? { note: row.text } : { left: row, right: row });
    }
  }
  flush();
  return result;
}
