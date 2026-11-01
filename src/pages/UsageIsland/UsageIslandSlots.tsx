import { useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
} from '@dnd-kit/sortable';
import { SortableCard } from '../../components/SortableCard';
import { useI18n } from '../../hooks/useI18n';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { USAGE_ISLAND_LIMIT, useUsageIslandStore } from '../../stores/usageIslandStore';
import { VoiceChannelCard } from '../VoiceInput/VoiceChannelCard';
import type { UsageSource } from './usageIslandData';

export function UsageIslandSlots({ sources }: { sources: UsageSource[] }) {
  const { t } = useI18n();
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const selected = useUsageIslandStore((state) => state.sources);
  const setSources = useUsageIslandStore((state) => state.setSources);
  const toggleSource = useUsageIslandStore((state) => state.toggleSource);
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const renderSlot = (index: number, handle?: ReactNode, overlay = false) => {
    const key = selected[index];
    const source = sources.find((entry) => entry.key === key);
    const detail = source?.kind === 'account' && hidden ? ACCOUNT_NAME_MASK : source?.detail;
    return (
      <VoiceChannelCard
        index={index}
        label={source?.name ?? String(index + 1)}
        filled={!!key}
        name={source?.name ?? t('voice.missingSource')}
        detail={detail ?? ''}
        icon={source?.icon ?? null}
        disabled={activeDragId !== null || overlay}
        active={false}
        handle={key ? handle : undefined}
        overlay={overlay}
        removeLabel={t('usageIsland.removeSource')}
        onRemove={() => toggleSource(key)}
      />
    );
  };
  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveDragId(null);
    if (!over || active.id === over.id) return;
    const from = selected.indexOf(String(active.id));
    const to = selected.indexOf(String(over.id));
    if (from >= 0 && to >= 0) setSources(arrayMove(selected, from, to));
  };
  const dragIndex = activeDragId === null ? -1 : selected.indexOf(activeDragId);
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      autoScroll={false}
      onDragStart={({ active }) => setActiveDragId(String(active.id))}
      onDragCancel={() => setActiveDragId(null)}
      onDragEnd={handleDragEnd}
    >
      <SortableContext items={selected} strategy={rectSortingStrategy}>
        <div
          className="usage-island-slots"
          role="list"
          aria-label={t('usageIsland.count')
            .replace('{n}', String(selected.length))
            .replace('{max}', String(USAGE_ISLAND_LIMIT))}
        >
          {Array.from({ length: USAGE_ISLAND_LIMIT }, (_, index) => {
            const id = selected[index] ?? `empty:${index}`;
            return (
              <SortableCard
                key={id}
                id={id}
                dragLabel={t('model.dragSort')}
                disabled={!selected[index] || selected.length < 2}
              >
                {(handle) => renderSlot(index, handle)}
              </SortableCard>
            );
          })}
        </div>
      </SortableContext>
      {createPortal(
        <DragOverlay
          dropAnimation={null}
          zIndex={1002}
          className="pointer-events-none font-sans text-cyber-text"
        >
          {dragIndex >= 0 ? renderSlot(dragIndex, undefined, true) : null}
        </DragOverlay>,
        document.body
      )}
    </DndContext>
  );
}
