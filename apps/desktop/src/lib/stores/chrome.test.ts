// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { chrome } from './chrome.svelte';

/**
 * The frame of the app: the sidebar is full or an icon rail, the hosts
 * inspector is open or not. Both are remembered on this device.
 */

beforeEach(() => {
  localStorage.clear();
  chrome.sidebarCollapsed = false;
  chrome.inspectorOpen = true;
  chrome.narrow = false;
});

afterEach(() => localStorage.clear());

describe('chrome', () => {
  it('toggles the sidebar and remembers the choice', () => {
    expect(chrome.railed).toBe(false);
    chrome.toggleSidebar();
    expect(chrome.sidebarCollapsed).toBe(true);
    expect(chrome.railed).toBe(true);
    expect(localStorage.getItem('termru.chrome.sidebarCollapsed')).toBe('1');
    chrome.toggleSidebar();
    expect(chrome.railed).toBe(false);
    expect(localStorage.getItem('termru.chrome.sidebarCollapsed')).toBe('0');
  });

  it('is a rail on a narrow window whatever the choice was', () => {
    chrome.narrow = true;
    expect(chrome.sidebarCollapsed).toBe(false);
    expect(chrome.railed).toBe(true);
  });

  it('toggles the inspector and remembers the choice', () => {
    chrome.toggleInspector();
    expect(chrome.inspectorOpen).toBe(false);
    expect(localStorage.getItem('termru.chrome.inspectorOpen')).toBe('0');
    chrome.toggleInspector();
    expect(chrome.inspectorOpen).toBe(true);
    expect(localStorage.getItem('termru.chrome.inspectorOpen')).toBe('1');
  });

  it('keeps working when storage refuses', () => {
    const real = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {
      throw new Error('blocked');
    };
    try {
      expect(() => chrome.toggleSidebar()).not.toThrow();
      expect(chrome.sidebarCollapsed).toBe(true);
    } finally {
      Storage.prototype.setItem = real;
    }
  });
});
