// Upstream helper with exponential backoff.
export async function fetchWithRetry(url, opts) {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      return await fetch(url, opts);
    } catch (err) {
      await sleep(2 ** attempt * 100);
    }
  }
  throw new Error("upstream unavailable");
}
