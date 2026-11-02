import { describe, expect, it } from 'vitest';
import { quotaColorClasses, quotaTextClass } from '../../utils/quotaColors';
import { QuotaPercent } from './QuotaPercent';

describe('QuotaPercent', () => {
  it('gives a whole quota group the color of the percent it shows', () => {
    expect(quotaTextClass(30.4)).toBe(quotaColorClasses.low.text);
    expect(quotaTextClass(30.5)).toBe(quotaColorClasses.healthy.text);
    expect(quotaTextClass(null)).toBe(quotaColorClasses.unknown.text);
  });

  it.each([
    [81.4, '81%', 'healthy'],
    [31, '31%', 'healthy'],
    [30.4, '30%', 'low'],
    [30, '30%', 'low'],
    [11, '11%', 'low'],
    [10, '10%', 'critical'],
    [0, '0%', 'critical'],
  ] as const)('shows %s remaining as %s in the %s color', (percent, text, tone) => {
    const { className, children } = QuotaPercent({ percent }).props;
    expect(children).toBe(text);
    expect(className).toBe(quotaColorClasses[tone].text);
  });

  it.each([null, undefined])(
    'keeps an unknown quota neutral instead of showing zero',
    (percent) => {
      const { className, children } = QuotaPercent({ percent }).props;
      expect(children).toBe('—');
      expect(className).toBe(quotaColorClasses.unknown.text);
    }
  );
});
