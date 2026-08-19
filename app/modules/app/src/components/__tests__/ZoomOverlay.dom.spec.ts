import type {UiZoomAction} from '@generated/UiZoomAction';
import {changeUiZoom} from '@app/commands/zoom';
import {flushPromises, mount} from '@vue/test-utils';
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest';
import ZoomOverlay from '../ZoomOverlay.vue';

const {mockLogError} = vi.hoisted(() => ({mockLogError: vi.fn()}));

vi.mock('@app/commands/zoom', () => ({changeUiZoom: vi.fn()}));
vi.mock('@app/log', () => ({getLogger: () => ({error: mockLogError})}));

const mockChangeUiZoom = vi.mocked(changeUiZoom);

/** Dispatch one keyboard zoom candidate to the window. */
function press(key: string, options: KeyboardEventInit = {}): void {
  window.dispatchEvent(new KeyboardEvent('keydown', {cancelable: true, key, ...options}));
}

/** Dispatch one mouse-wheel zoom candidate to the window. */
function wheel(deltaY: number, options: WheelEventInit = {}): void {
  window.dispatchEvent(new WheelEvent('wheel', {cancelable: true, deltaY, ...options}));
}

/** Return a distinct backend response for each zoom action. */
function zoomResponse(action: UiZoomAction): Promise<{factor: number}> {
  const factor = action === 'increase' ? 1.1 : action === 'decrease' ? 0.9 : 1;
  return Promise.resolve({factor});
}

describe('zoomOverlay', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
    mockChangeUiZoom.mockImplementation(zoomResponse);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('maps Windows keyboard shortcuts to backend zoom actions', async () => {
    const wrapper = mount(ZoomOverlay);

    press('+', {ctrlKey: true});
    press('=', {ctrlKey: true});
    press('-', {ctrlKey: true});
    press('0', {ctrlKey: true});
    press('A', {ctrlKey: true});
    press('+');
    press('+', {metaKey: true});
    press('+', {altKey: true, ctrlKey: true});
    await flushPromises();

    expect(mockChangeUiZoom.mock.calls.map(([action]) => action)).toEqual([
      'increase',
      'increase',
      'decrease',
      'reset',
    ]);
    expect(wrapper.get('.zoom-overlay').text()).toBe('100%');
    wrapper.unmount();
  });

  it('uses Command rather than Control on macOS', async () => {
    vi.spyOn(navigator, 'platform', 'get').mockReturnValue('MacIntel');
    const wrapper = mount(ZoomOverlay);

    press('+', {metaKey: true});
    press('+', {ctrlKey: true});
    press('+', {ctrlKey: true, metaKey: true});
    await flushPromises();

    expect(mockChangeUiZoom).toHaveBeenCalledOnce();
    expect(mockChangeUiZoom).toHaveBeenCalledWith('increase');
    wrapper.unmount();
  });

  it('maps modified wheel movement and ignores unrelated wheel events', async () => {
    const wrapper = mount(ZoomOverlay);

    wheel(-100, {ctrlKey: true});
    wheel(100, {ctrlKey: true});
    wheel(0, {ctrlKey: true});
    wheel(-100);
    wheel(-100, {metaKey: true});
    wheel(-100, {altKey: true, ctrlKey: true});
    await flushPromises();

    expect(mockChangeUiZoom.mock.calls.map(([action]) => action)).toEqual(['increase', 'decrease']);
    wrapper.unmount();
  });

  it('keeps the latest percentage visible for two seconds', async () => {
    vi.useFakeTimers();
    const wrapper = mount(ZoomOverlay);

    press('+', {ctrlKey: true});
    await flushPromises();
    expect(wrapper.get('.zoom-overlay').text()).toBe('110%');

    await vi.advanceTimersByTimeAsync(1500);
    press('-', {ctrlKey: true});
    await flushPromises();
    await vi.advanceTimersByTimeAsync(1500);
    expect(wrapper.get('.zoom-overlay').text()).toBe('90%');

    await vi.advanceTimersByTimeAsync(500);
    expect(wrapper.find('.zoom-overlay').exists()).toBe(false);
    wrapper.unmount();
  });

  it('logs rejected backend changes and removes listeners on unmount', async () => {
    mockChangeUiZoom.mockRejectedValueOnce('zoom unavailable');
    const wrapper = mount(ZoomOverlay);

    press('+', {ctrlKey: true});
    await flushPromises();
    expect(mockLogError).toHaveBeenCalledWith('Failed to change application zoom:', 'zoom unavailable');

    wrapper.unmount();
    mockChangeUiZoom.mockClear();
    press('+', {ctrlKey: true});
    wheel(-100, {ctrlKey: true});
    expect(mockChangeUiZoom).not.toHaveBeenCalled();
  });
});
