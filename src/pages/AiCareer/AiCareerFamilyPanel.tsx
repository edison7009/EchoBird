import { ExternalLink, RotateCcw } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useToast } from '../../components/Toast';
import { useAiCareerStore } from '../../stores/aiCareerStore';
import { useToolsStore } from '../../stores/toolsStore';
import { openExternal } from '../../api/tauri';
import { BrandRow } from '../../components/BrandRow';
import { CAREER_FAMILIES, FamilyIcon } from './familyIcons';
import '../../components/brandCards.css';

export function AiCareerFamilyPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const tools = useToolsStore((s) => s.detectedTools);
  const selectedFamily = useAiCareerStore((s) => s.selectedFamily);
  const setSelectedFamily = useAiCareerStore((s) => s.setSelectedFamily);
  const cardClass = (selected: boolean) =>
    'sidebar-card w-full h-14 shrink-0 flex border-2 rounded-card ' +
    (selected ? 'is-selected' : 'border-transparent');
  const openWebsite = async (url: string) => {
    try {
      await openExternal(url);
    } catch {
      showToast('error', t('aiCareer.actionFailed'));
    }
  };

  return (
    <nav
      aria-label={t('aiCareer.families')}
      className="flex-1 min-h-0 px-2 pb-2 overflow-y-auto slim-scroll space-y-2 cursor-default"
    >
      <button
        type="button"
        aria-pressed={selectedFamily === null}
        onClick={() => setSelectedFamily(null)}
        className="brand-new-row rounded-card text-left"
      >
        <span className="brand-deck-plus [--brand-action-icon-rotation:-60deg]" aria-hidden="true">
          <RotateCcw className="origin-center rotate-[66deg]" size={14} strokeWidth={3} />
        </span>
        <span className="truncate">{t('aiCareer.overview')}</span>
      </button>
      {CAREER_FAMILIES.map((family) => (
        <FamilyRow
          key={family.id}
          family={family}
          className={cardClass(selectedFamily === family.id)}
          selected={selectedFamily === family.id}
          onSelect={() => setSelectedFamily(family.id)}
          label={`${family.name} ${t('aiCareer.familySuffix')}`}
          website={tools.find((tool) => tool.id === family.toolId)?.website}
          websiteLabel={t('accountCenter.website').replace('{name}', family.name)}
          onOpenWebsite={(url) => void openWebsite(url)}
        />
      ))}
    </nav>
  );
}

/** A family as a list row in its own colour, read from its icon. */
function FamilyRow({
  family,
  className,
  selected,
  onSelect,
  label,
  website,
  websiteLabel,
  onOpenWebsite,
}: {
  family: (typeof CAREER_FAMILIES)[number];
  className: string;
  selected: boolean;
  onSelect: () => void;
  label: string;
  website?: string;
  websiteLabel: string;
  onOpenWebsite: (url: string) => void;
}) {
  return (
    <BrandRow
      iconSrc={'./icons/tools/' + family.toolId + '.' + family.format}
      className={className}
    >
      <button
        type="button"
        aria-pressed={selected}
        onClick={onSelect}
        className="flex-1 min-w-0 flex items-center gap-3 px-[11px] text-left"
      >
        <FamilyIcon family={family.id} className="w-8 h-8" />
        <span className="text-base font-bold text-cyber-text truncate">{label}</span>
      </button>
      {website && (
        <button
          type="button"
          aria-label={websiteLabel}
          onClick={() => onOpenWebsite(website)}
          className="flex-shrink-0 px-3 text-cyber-text-secondary hover:text-cyber-text"
        >
          <ExternalLink size={20} />
        </button>
      )}
    </BrandRow>
  );
}
