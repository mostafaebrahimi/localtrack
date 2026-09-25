import { describe, expect, it } from 'vitest';

import { clamp, domainOf, envelope, isAck, LIMITS, sanitizeUrl } from '../index';

describe('sanitizeUrl', () => {
  const sensitive = 'https://user:pw@example.com/orders/183?token=abc&page=2#payment';

  it('strips credentials, query and fragment by default', () => {
    expect(sanitizeUrl(sensitive, 'PATH_WITHOUT_QUERY')).toBe('https://example.com/orders/183');
  });

  it('matches the spec §138 privacy case', () => {
    const out = sanitizeUrl('https://example.com/login?password=secret&token=123', 'PATH_WITHOUT_QUERY');
    expect(out).toBe('https://example.com/login');
    expect(out).not.toContain('secret');
    expect(out).not.toContain('123');
  });

  it('supports domain-only storage', () => {
    expect(sanitizeUrl(sensitive, 'DOMAIN_ONLY')).toBe('https://example.com');
  });

  it('keeps the query only under FULL_URL and never the credentials', () => {
    const out = sanitizeUrl(sensitive, 'FULL_URL');
    expect(out).toContain('token=abc');
    expect(out).not.toContain('user:pw');
  });

  it('drops non-web schemes', () => {
    expect(sanitizeUrl('chrome://extensions', 'FULL_URL')).toBeUndefined();
    expect(sanitizeUrl('file:///etc/passwd', 'FULL_URL')).toBeUndefined();
    expect(sanitizeUrl('not a url', 'FULL_URL')).toBeUndefined();
  });
});

describe('domainOf', () => {
  it('normalizes the host', () => {
    expect(domainOf('https://WWW.GitHub.com/a/b')).toBe('github.com');
    expect(domainOf('chrome://newtab')).toBeUndefined();
  });
});

describe('clamp', () => {
  it('collapses whitespace and clamps length', () => {
    expect(clamp('  Save   Invoice ', 120)).toBe('Save Invoice');
    expect(clamp('x'.repeat(500), LIMITS.interactionLabel)).toHaveLength(LIMITS.interactionLabel);
    expect(clamp('   ', 10)).toBeUndefined();
  });
});

describe('envelope', () => {
  it('stamps the protocol version', () => {
    const message = envelope('browser.activity', { a: 1 }, 'id-1');
    expect(message.version).toBe(1);
    expect(message.type).toBe('browser.activity');
    expect(message.sentAt).toBeGreaterThan(0);
  });

  it('narrows ack responses', () => {
    expect(isAck({ version: 1, messageId: 'a', type: 'ack', receivedAt: 1, success: true })).toBe(true);
    expect(
      isAck({ version: 1, messageId: 'a', type: 'error', error: { code: 'X', message: 'y' } }),
    ).toBe(false);
  });
});
