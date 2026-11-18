import { motherMaintenanceLabels } from '../../data/motherMaintenanceLabels';
import residualWorkflow from './prompts/residualCleanup.md?raw';
import diskWorkflow from './prompts/diskSpace.md?raw';

/** Keep both shortcuts editable; attach a workflow only on explicit native send. */
export function maintenanceMessage(
  message: string,
  labels: { residuals: string; diskSpace: string },
  serverId: string,
  locale: string
): string {
  const selected = [
    {
      labels: [labels.residuals, ...Object.values(motherMaintenanceLabels.residuals)],
      workflow: residualWorkflow,
      tag: 'echobird-residual-cleanup',
    },
    {
      labels: [labels.diskSpace, ...Object.values(motherMaintenanceLabels.diskSpace)],
      workflow: diskWorkflow,
      tag: 'echobird-disk-space',
    },
  ].find((item) => item.labels.some((label) => label && message.trimStart().startsWith(label)));
  if (!selected) return message;
  const target =
    serverId === 'local' ? 'local computer' : `selected SSH server ${JSON.stringify(serverId)}`;
  return `${message}\n\n<${selected.tag}>\nTarget machine: ${target}. Response language: ${locale}.\n${selected.workflow}\n</${selected.tag}>`;
}
