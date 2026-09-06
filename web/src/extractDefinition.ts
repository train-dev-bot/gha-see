function indentOf(line: string): number {
  return line.match(/^\s*/)?.[0].length ?? 0;
}

function normalizedLines(source: string): string[] {
  return source.replace(/\r\n?/g, "\n").split("\n");
}

function jobBounds(
  source: string,
  jobId: string,
): { lines: string[]; start: number; end: number } | null {
  const lines = normalizedLines(source);
  const jobsIndex = lines.findIndex((line) => /^jobs:\s*(?:#.*)?$/.test(line));
  if (jobsIndex < 0) return null;

  const keyPattern = /^(\s*)([A-Za-z0-9_-]+):(?:\s.*)?$/;
  const siblingKeyPattern =
    /^(\s*)(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[A-Za-z0-9_-]+):(?:\s.*)?$/;
  let jobIndent: number | null = null;
  let start = -1;

  for (let i = jobsIndex + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.trim() === "") continue;

    const indent = indentOf(line);
    if (indent === 0) break;
    if (jobIndent == null) jobIndent = indent;
    if (line.trimStart().startsWith("#")) continue;

    const match = line.match(keyPattern);
    if (!match) continue;

    if (indent !== jobIndent) continue;

    if (
      match[2] === jobId &&
      new RegExp(`^\\s*${jobId}:\\s*(?:#.*)?$`).test(line)
    ) {
      start = i;
      break;
    }
  }

  if (start < 0 || jobIndent == null) return null;

  let end = lines.length;
  for (let i = start + 1; i < lines.length; i += 1) {
    const match = lines[i].match(siblingKeyPattern);
    if (match && match[1].length === jobIndent) {
      end = i;
      break;
    }
    if (
      lines[i].trim() !== "" &&
      !lines[i].trimStart().startsWith("#") &&
      indentOf(lines[i]) < jobIndent
    ) {
      end = i;
      break;
    }
  }

  return { lines, start, end };
}

export function extractJobYaml(source: string, jobId: string): string | null {
  const bounds = jobBounds(source, jobId);
  if (!bounds) return null;
  return bounds.lines.slice(bounds.start, bounds.end).join("\n");
}

export function extractStepYaml(
  source: string,
  jobId: string,
  stepIdx: number,
): string | null {
  if (!Number.isInteger(stepIdx) || stepIdx < 0) return null;

  const bounds = jobBounds(source, jobId);
  if (!bounds) return null;

  let stepsIndex = -1;
  let stepsIndent = -1;
  for (let i = bounds.start + 1; i < bounds.end; i += 1) {
    if (/^\s*steps:\s*(?:#.*)?$/.test(bounds.lines[i])) {
      stepsIndex = i;
      stepsIndent = indentOf(bounds.lines[i]);
      break;
    }
  }
  if (stepsIndex < 0) return null;

  const itemPattern = /^(\s*)-\s/;
  let itemIndent: number | null = null;
  const itemStarts: number[] = [];
  for (let i = stepsIndex + 1; i < bounds.end; i += 1) {
    const line = bounds.lines[i];
    if (
      line.trim() !== "" &&
      !line.trimStart().startsWith("#") &&
      indentOf(line) <= stepsIndent
    ) {
      break;
    }

    const match = line.match(itemPattern);
    if (!match) continue;
    if (itemIndent == null) itemIndent = match[1].length;
    if (match[1].length === itemIndent) itemStarts.push(i);
  }

  const start = itemStarts[stepIdx];
  if (start == null) return null;
  const end = itemStarts[stepIdx + 1] ?? bounds.end;
  return bounds.lines.slice(start, end).join("\n");
}
