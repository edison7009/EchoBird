import type {
  RouterStatisticsCatalog,
  RouterStatisticsCounts,
  RouterStatisticsSnapshot,
} from '../api/routerStatistics';

export const emptyStatisticsCounts = (): RouterStatisticsCounts => ({
  attempts: 0,
  completed: 0,
  failed: 0,
  cancelled: 0,
  protections: [0, 0, 0, 0, 0],
  errors: [0, 0, 0, 0, 0, 0],
});

export interface StatisticsDisplayRow {
  id: string;
  account: string;
  name: string;
  model: string;
  provider: string;
  enabled: boolean;
  archived: boolean;
  lastUsedAt: number;
  groups: string[];
  models: { name: string; attempts: number }[];
  counts: RouterStatisticsCounts;
}

export function statisticsRows(
  snapshot: RouterStatisticsSnapshot | null,
  catalog: RouterStatisticsCatalog | null,
  group: string
): StatisticsDisplayRow[] {
  const sources = new Map((catalog?.sources ?? []).map((source) => [source.id, source]));
  const rows = new Map<string, StatisticsDisplayRow>();
  for (const saved of snapshot?.rows ?? []) {
    if (group && saved.group !== group) continue;
    const source = sources.get(saved.id) ?? {
      ...saved,
      name: '',
      modelName: '',
      provider: '',
      enabled: false,
    };
    const id = source.account;
    let row = rows.get(id);
    if (!row) {
      row = {
        id,
        account: source.account,
        name: source.name,
        model: source.modelName,
        provider: source.provider,
        enabled: false,
        archived: !source.name,
        lastUsedAt: 0,
        groups: [],
        models: [],
        counts: emptyStatisticsCounts(),
      };
      rows.set(id, row);
    }
    if (row.archived && source.name) {
      row.name = source.name;
      row.model = source.modelName;
      row.provider = source.provider;
      row.archived = false;
    }
    row.enabled ||= source.enabled;
    row.lastUsedAt = Math.max(row.lastUsedAt, saved.lastUsedAt);
    if (!row.groups.includes(source.group)) row.groups.push(source.group);
    const modelName = source.modelName;
    let model = row.models.find((model) => model.name === modelName);
    if (!model) {
      model = { name: modelName, attempts: 0 };
      row.models.push(model);
    }
    const counts = saved.counts;
    if (counts.usage && counts.usage.reportedResponses > 0) {
      const usage = (row.counts.usage ??= {
        inputTokens: 0,
        outputTokens: 0,
        totalTokens: 0,
        reportedResponses: 0,
      });
      usage.inputTokens += counts.usage.inputTokens;
      usage.outputTokens += counts.usage.outputTokens;
      usage.totalTokens += counts.usage.totalTokens;
      usage.reportedResponses += counts.usage.reportedResponses;
    }
    model.attempts += counts.attempts;
    row.counts.attempts += counts.attempts;
    row.counts.completed += counts.completed;
    row.counts.failed += counts.failed;
    row.counts.cancelled += counts.cancelled;
    row.counts.protections = row.counts.protections.map(
      (value, index) => value + (counts.protections[index] ?? 0)
    );
    row.counts.errors = row.counts.errors.map(
      (value, index) => value + (counts.errors[index] ?? 0)
    );
  }
  return [...rows.values()].sort(
    (a, b) =>
      b.counts.attempts - a.counts.attempts ||
      b.lastUsedAt - a.lastUsedAt ||
      a.id.localeCompare(b.id)
  );
}
