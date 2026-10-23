// Token bucket limiter shared by the queue handlers.
export class RateLimiter {
  constructor(capacity, refillPerSecond) {
    this.capacity = capacity;
    this.tokens = capacity;
    this.refill = refillPerSecond;
    this.last = Date.now();
  }

  take(n = 1) {
    this.refillTokens();
    if (this.tokens < n) return false;
    this.tokens -= n;
    return true;
  }

  refillTokens() {
    const now = Date.now();
    this.tokens = Math.min(this.capacity, this.tokens + ((now - this.last) / 1000) * this.refill);
    this.last = now;
  }
}
