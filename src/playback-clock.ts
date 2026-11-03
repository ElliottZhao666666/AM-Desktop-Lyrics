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
    this.playing = false;
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
    if (!this.initialized || discontinuity) this.position = this.targetPosition;
    else if (!this.playing) this.targetPosition = this.position;
    this.initialized = true;
    this.lastTime = now;
    return this.position;
  }

  tick(now: number) {
    const elapsed = Math.max(0, now - this.lastTime);
    if (this.playing && elapsed > 0) {
      const predicted = this.position + elapsed * this.rate;
      const error = this.targetAt(now) - predicted;
      // 小误差慢调，大误差快收敛；向后的修正最多暂缓推进，不反向倒放。
      const magnitude = Math.abs(error);
      const timeConstant = magnitude >= 500 ? 100 : magnitude >= 120 ? 250 : 800;
      const correctionRate = magnitude >= 500 ? 2 : magnitude >= 120 ? 0.6 : 0.2;
      const correction = magnitude < 8 ? 0 : Math.max(-elapsed * this.rate * Math.min(0.95, correctionRate),
        Math.min(elapsed * this.rate * correctionRate, error * (1 - Math.exp(-elapsed / timeConstant))));
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
