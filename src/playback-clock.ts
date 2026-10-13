/** SMTC 用于校准；每一帧由单调时钟推进。小误差改变速度，不直接改位置。 */
export class PlaybackClock {
  private position = 0;
  private targetPosition = 0;
  private lastTime = 0;
  private targetTime = 0;
  private playing = false;
  private rate = 1;
  private duration = 0;
  private initialized = false;

  reset(now: number) {
    this.position = this.targetPosition = this.duration = 0;
    this.lastTime = this.targetTime = now;
    this.rate = 1;
    this.initialized = false;
  }

  setPlaying(playing: boolean, now: number) {
    this.tick(now);
    this.targetPosition = this.targetAt(now);
    this.targetTime = now;
    this.playing = playing;
  }

  sync(position: number, duration: number, rate: number, now: number, discontinuity = false) {
    this.tick(now);
    this.rate = Number.isFinite(rate) && rate >= 0 ? rate : 1;
    this.duration = Number.isFinite(duration) && duration > 0 ? duration : this.duration;
    this.targetPosition = this.limit(position);
    this.targetTime = now;
    if (!this.initialized || discontinuity || !this.playing) this.position = this.targetPosition;
    this.initialized = true;
    this.lastTime = now;
    return this.position;
  }

  tick(now: number) {
    const elapsed = Math.max(0, now - this.lastTime);
    if (this.playing && elapsed > 0) {
      const predicted = this.position + elapsed * this.rate;
      const error = this.targetAt(now) - predicted;
      // 指数收敛，并限制在原播放速度的 80%～120%，保持正常播放时始终向前。
      const correctionLimit = elapsed * this.rate * 0.2;
      const correction = Math.abs(error) < 8 ? 0 : Math.max(-correctionLimit, Math.min(correctionLimit, error * (1 - Math.exp(-elapsed / 800))));
      this.position = this.limit(predicted + correction);
    }
    this.lastTime = Math.max(this.lastTime, now);
    return this.position;
  }

  private targetAt(now: number) {
    return this.targetPosition + (this.playing ? Math.max(0, now - this.targetTime) * this.rate : 0);
  }
  private limit(position: number) {
    const value = Number.isFinite(position) ? Math.max(0, position) : 0;
    return this.duration > 0 ? Math.min(value, this.duration) : value;
  }
}
