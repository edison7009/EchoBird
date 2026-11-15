import { expect, it } from 'vitest';
import {
  accountAvailable,
  accountProxySources,
  nativeAccountApplications,
} from './useAccountCapability';
import { accountProxyProviders } from '../api/accountProxy';

it('provides built-in accounts without installing or enabling a plugin', () => {
  for (const application of nativeAccountApplications) {
    expect(accountAvailable(application)).toBe(true);
  }
  expect(accountProxySources()).toEqual(accountProxyProviders);
});
