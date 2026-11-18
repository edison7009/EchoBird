const ordinaryUrl = new URL('../../assets/pets/level-up.wav', import.meta.url).href;
const milestoneUrl = new URL('../../assets/pets/milestone.mp3', import.meta.url).href;

function createAudioClips(urls: string[], volume: number) {
  let clips: HTMLAudioElement[] = [];
  let playback = 0;
  function stop() {
    playback += 1;
    for (const clip of clips) {
      clip.pause();
      clip.currentTime = 0;
      clip.volume = volume;
    }
  }
  return {
    prepare() {
      if (clips.length) return;
      clips = urls.map((url) => {
        const clip = new Audio(url);
        clip.preload = 'auto';
        clip.volume = volume;
        return clip;
      });
    },
    // Unlock delayed playback inside the opening gesture without an audible cue.
    prime() {
      const clip = clips[0];
      if (!clip) return;
      const revision = playback;
      clip.volume = 0;
      const settle = () => {
        if (revision !== playback) return;
        clip.pause();
        clip.currentTime = 0;
        clip.volume = volume;
      };
      void clip.play().then(settle, settle);
    },
    play(index: number) {
      stop();
      void clips[index]?.play().catch(() => {});
    },
    stop,
    dispose() {
      stop();
      for (const clip of clips) {
        clip.removeAttribute('src');
        clip.load();
      }
      clips = [];
    },
  };
}

export function createPetAudio() {
  const clips = createAudioClips([ordinaryUrl, milestoneUrl], 0.55);
  return { ...clips, play: (milestone: boolean) => clips.play(milestone ? 1 : 0) };
}

export function createRewardAudio() {
  const clips = createAudioClips(
    [new URL('../../assets/pets/pack-tear.wav', import.meta.url).href],
    0.4
  );
  return { ...clips, play: () => clips.play(0) };
}
