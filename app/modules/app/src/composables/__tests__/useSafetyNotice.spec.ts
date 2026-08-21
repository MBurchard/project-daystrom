import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useSafetyNotice} from '../useSafetyNotice';

const mockIsRequired = vi.hoisted(() => vi.fn());
const mockGetContext = vi.hoisted(() => vi.fn());
const mockAcknowledge = vi.hoisted(() => vi.fn());
const mockLog = vi.hoisted(() => ({error: vi.fn()}));

vi.mock('@app/commands/safetyNotice', () => ({
  isSafetyNoticeRequired: mockIsRequired,
  getSafetyNoticeContext: mockGetContext,
  acknowledgeSafetyNotice: mockAcknowledge,
}));
vi.mock('@app/log', () => ({getLogger: vi.fn().mockReturnValue(mockLog)}));

describe('useSafetyNotice', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAcknowledge.mockResolvedValue(undefined);
    mockGetContext.mockResolvedValue({
      platform: 'macos',
      cleanupPaths: ['/Users/Test/Library/Application Support/mbur.project-daystrom'],
      modLibraryPath: null,
    });
  });

  it('loads and acknowledges the backend-owned requirement', async () => {
    mockIsRequired.mockResolvedValue(true);
    const notice = useSafetyNotice();

    notice.init();
    await vi.waitFor(() => expect(notice.required.value).toBe(true));
    expect(notice.context.value?.platform).toBe('macos');
    notice.acknowledge();
    expect(notice.pending.value).toBe(true);
    notice.acknowledge();
    expect(mockAcknowledge).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(notice.required.value).toBe(false));
    expect(notice.pending.value).toBe(false);
    expect(notice.failed.value).toBe(false);
  });

  it('logs requirement and acknowledgement failures without clearing the notice', async () => {
    mockIsRequired.mockRejectedValue('load failed');
    mockGetContext.mockRejectedValue('context failed');
    mockAcknowledge.mockRejectedValue('save failed');
    const notice = useSafetyNotice();

    notice.init();
    notice.acknowledge();

    await vi.waitFor(() => expect(mockLog.error).toHaveBeenCalledTimes(3));
    expect(notice.required.value).toBe(true);
    expect(notice.context.value).toBeNull();
    expect(notice.pending.value).toBe(false);
    expect(notice.failed.value).toBe(true);
  });
});
