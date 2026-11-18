import { afterEach, expect, it, vi } from 'vitest';
import { createPetAudio, createRewardAudio } from './petAudio';

afterEach(() => vi.unstubAllGlobals());
it('loads only after explicit preparation, chooses one cue and releases media on exit', async () => {
  const clips: {
    play: ReturnType<typeof vi.fn>;
    pause: ReturnType<typeof vi.fn>;
    removeAttribute: ReturnType<typeof vi.fn>;
    load: ReturnType<typeof vi.fn>;
    currentTime: number;
  }[] = [];
  vi.stubGlobal(
    'Audio',
    vi.fn(function () {
      const clip = {
        play: vi.fn(() => Promise.resolve()),
        pause: vi.fn(),
        removeAttribute: vi.fn(),
        load: vi.fn(),
        currentTime: 0,
      };
      clips.push(clip);
      return clip;
    })
  );
  const sound = createPetAudio();
  expect(clips).toHaveLength(0);
  sound.prepare();
  sound.prepare();
  expect(clips).toHaveLength(2);
  sound.play(false);
  expect(clips[0].play).toHaveBeenCalledTimes(1);
  expect(clips[1].play).not.toHaveBeenCalled();
  sound.play(true);
  expect(clips[1].play).toHaveBeenCalledTimes(1);
  sound.dispose();
  expect(
    clips.every(
      (clip) =>
        clip.pause.mock.calls.length > 0 &&
        clip.removeAttribute.mock.calls.length === 1 &&
        clip.load.mock.calls.length === 1
    )
  ).toBe(true);
});

it('prepares the local tear cue once on demand, catches blocked playback and releases it', async () => {
  const clip = {
    play: vi.fn(() => Promise.reject(new Error('blocked'))),
    pause: vi.fn(),
    currentTime: 0,
    removeAttribute: vi.fn(),
    load: vi.fn(),
  };
  const Audio = vi.fn(function (_url: string) {
    return clip;
  });
  vi.stubGlobal('Audio', Audio);
  const sound = createRewardAudio();
  expect(Audio).not.toHaveBeenCalled();
  sound.prepare();
  sound.prepare();
  sound.play();
  await Promise.resolve();
  expect(Audio).toHaveBeenCalledOnce();
  expect(Audio.mock.calls[0][0]).toContain('pack-tear.wav');
  expect(clip.play).toHaveBeenCalledOnce();
  sound.dispose();
  expect(clip.pause).toHaveBeenCalled();
  expect(clip.removeAttribute).toHaveBeenCalledWith('src');
  expect(clip.load).toHaveBeenCalledOnce();
});

it('silently primes the gesture and does not let a late unlock stop the delayed tear cue', async () => {
  let unlock!: () => void;
  const pending = new Promise<void>((resolve) => {
    unlock = resolve;
  });
  const clip = {
    volume: 1,
    currentTime: 0,
    play: vi.fn(() => Promise.resolve()).mockImplementationOnce(() => pending),
    pause: vi.fn(),
    removeAttribute: vi.fn(),
    load: vi.fn(),
  };
  vi.stubGlobal(
    'Audio',
    vi.fn(function () {
      return clip;
    })
  );
  const sound = createRewardAudio();
  sound.prepare();
  sound.prime();
  expect(clip.volume).toBe(0);
  sound.play();
  expect(clip.volume).toBe(0.4);
  clip.currentTime = 0.15;
  const pauses = clip.pause.mock.calls.length;
  unlock();
  await Promise.resolve();
  expect(clip.pause).toHaveBeenCalledTimes(pauses);
  expect(clip.currentTime).toBe(0.15);
  sound.dispose();
});
