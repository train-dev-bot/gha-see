export function preservedActiveIndex(
  workflows: Array<{ index: number; path: string }>,
  previous: { index: number; path: string } | null | undefined,
): number {
  const first = workflows[0]?.index ?? 0;
  if (!previous || workflows.length === 0) return first;
  const byPath = workflows.find((workflow) => workflow.path === previous.path);
  if (byPath) return byPath.index;
  const byIndex = workflows.find((workflow) => workflow.index === previous.index);
  return byIndex?.index ?? first;
}
