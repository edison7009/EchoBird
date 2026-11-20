import { useEffect, useRef, useState } from 'react';
import type { VoiceBinding } from '../../api/voiceInput';
import { suspendVoiceShortcut } from '../../api/voiceInput';
import { useI18n } from '../../hooks/useI18n';

const modifierCode = /^(Control|Alt|Shift|Meta)(Left|Right)$/;

export function bindingFromEvent(
  event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'> &
    Partial<Pick<KeyboardEvent, 'key' | 'location'>>
): VoiceBinding {
  let code = event.code;
  // Remote and injected keyboard events can omit the physical code.
  if (!code || code === 'Unidentified') {
    const key = event.key ?? '';
    code = /^[a-z]$/i.test(key)
      ? `Key${key.toUpperCase()}`
      : /^\d$/.test(key)
        ? `Digit${key}`
        : /^(Control|Alt|Shift|Meta)$/.test(key)
          ? `${key}${event.location === 2 ? 'Right' : 'Left'}`
          : key === ' '
            ? 'Space'
            : key;
  }
  return {
    code,
    modifiers: [
      ...(event.ctrlKey && !code.startsWith('Control') ? ['Control'] : []),
      ...(event.altKey && !code.startsWith('Alt') ? ['Alt'] : []),
      ...(event.shiftKey && !code.startsWith('Shift') ? ['Shift'] : []),
      ...(event.metaKey && !code.startsWith('Meta') ? ['Meta'] : []),
    ],
  };
}

export function formatBinding(
  binding: VoiceBinding,
  platform = navigator.platform,
  sides = { left: 'Left', right: 'Right' }
): string {
  const mac = /Mac/i.test(platform);
  const linux = /Linux/i.test(platform);
  const names: Record<string, string> = {
    Control: mac ? '⌃' : 'Ctrl',
    Alt: mac ? '⌥' : 'Alt',
    Shift: mac ? '⇧' : 'Shift',
    Meta: mac ? '⌘' : linux ? 'Super' : 'Win',
  };
  const base = binding.code.replace(/(Left|Right)$/, '');
  const key = modifierCode.test(binding.code)
    ? `${binding.code.endsWith('Right') ? sides.right : sides.left} ${names[base]}`
    : binding.code.replace(/^(Key|Digit)/, '');
  return [...binding.modifiers.map((modifier) => names[modifier] ?? modifier), key].join(
    mac ? ' ' : ' + '
  );
}

export function ShortcutRecorder({
  binding,
  disabled,
  active,
  enabled = true,
  onSave,
}: {
  binding: VoiceBinding;
  disabled: boolean;
  active: boolean;
  enabled?: boolean;
  onSave: (binding: VoiceBinding) => Promise<boolean>;
}) {
  const { t } = useI18n();
  const [recording, setRecording] = useState(false);
  const recorder = useRef<HTMLButtonElement>(null);
  const pending = useRef<VoiceBinding | null>(null);
  const committing = useRef(false);
  useEffect(() => {
    if (!recording || !active) return;
    recorder.current?.focus();
    void suspendVoiceShortcut(true);
    return () => {
      void suspendVoiceShortcut(false);
    };
  }, [recording, active]);
  useEffect(() => {
    if (!active) {
      pending.current = null;
    }
  }, [active]);
  const finish = async (value?: VoiceBinding) => {
    if (committing.current) return;
    committing.current = true;
    pending.current = null;
    if (value) await onSave(value);
    setRecording(false);
    committing.current = false;
  };
  if (!recording || !active)
    return (
      <button
        type="button"
        className="voice-key"
        disabled={disabled}
        onClick={() => setRecording(true)}
      >
        {enabled
          ? formatBinding(binding, navigator.platform, {
              left: t('voice.leftSide'),
              right: t('voice.rightSide'),
            })
          : t('voice.enableShortcut')}
      </button>
    );
  return (
    <button
      ref={recorder}
      type="button"
      className="voice-key voice-shortcut-listening"
      onBlur={() => void finish()}
      onKeyDown={(event) => {
        if (event.key === 'Tab') {
          void finish();
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        if (event.key === 'Escape') {
          void finish();
          return;
        }
        if (event.repeat) return;
        const next = bindingFromEvent(event);
        if (modifierCode.test(next.code)) pending.current = next;
        else void finish(next);
      }}
      onKeyUp={(event) => {
        event.preventDefault();
        const value = pending.current;
        if (value?.code === bindingFromEvent(event).code && value.modifiers.length === 0)
          void finish(value);
      }}
    >
      {t('voice.recordShortcut')}
    </button>
  );
}
