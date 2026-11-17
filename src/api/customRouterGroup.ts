import * as modelsApi from './models';
import * as routerApi from './smartRouter';
import type { ModelConfig } from './types';

export const MAX_ROUTER_GROUPS = 7;
export const ROUTER_GROUP_LIMIT = 'SMART_ROUTER_GROUP_LIMIT';
export interface CustomSwitchChoice {
  existing?: ModelConfig;
  input: Parameters<typeof modelsApi.addModel>[0];
}

// Use the same model and router APIs as their existing single-item editors.
// Only records created by this operation may be rolled back.
export async function createCustomRouterGroup(
  choices: CustomSwitchChoice[],
  defaultName: string,
  isCurrent: () => boolean,
  name?: string
): Promise<routerApi.SmartRouterConfig> {
  const check = () => {
    if (!isCurrent()) throw new Error('CUSTOM_SWITCH_CANCELLED');
  };
  const created: string[] = [];
  let group: routerApi.SmartRouterConfig | undefined;
  try {
    const groups = await routerApi.listSmartRouters();
    check();
    if (groups.length >= MAX_ROUTER_GROUPS) throw new Error(ROUTER_GROUP_LIMIT);
    if (!choices.length) throw new Error('CUSTOM_SWITCH_EMPTY');
    const ids: string[] = [];
    for (const choice of choices) {
      check();
      const model = choice.existing ?? (await modelsApi.addModel(choice.input));
      if (!choice.existing) created.push(model.internalId);
      ids.push(model.internalId);
      check();
    }
    const currentGroups = await routerApi.listSmartRouters();
    check();
    if (currentGroups.length >= MAX_ROUTER_GROUPS) throw new Error(ROUTER_GROUP_LIMIT);
    const port = await routerApi.getSmartRouterAvailablePort();
    check();
    group = await routerApi.saveSmartRouter({
      name: name ?? `${defaultName} #${currentGroups.length + 1}`,
      modelId: 'auto',
      port,
      apiKey: `eb-local-${crypto.randomUUID().replace(/-/g, '')}`,
    });
    check();
    const saved = await routerApi.setSmartRouterCandidates([...new Set(ids)], group.id);
    check();
    if (ids.some((id) => !saved.candidateIds.includes(id)))
      throw new Error('CUSTOM_SWITCH_REJECTED');
    return saved;
  } catch (error) {
    if (group) {
      // Preserve referenced models if cleanup of the new group cannot complete.
      await routerApi.deleteSmartRouter(group.id);
    }
    let cleanupError: unknown;
    for (const id of created.reverse()) {
      try {
        await modelsApi.deleteModel(id);
      } catch (failure) {
        cleanupError ??= failure;
      }
    }
    throw cleanupError ?? error;
  }
}
