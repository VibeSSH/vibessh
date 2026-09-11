/**
 * Returns the error a promise rejected with.
 *
 * Used instead of `expect(promise).rejects` throughout: in this vitest +
 * jsdom setup that matcher lets the rejection escape as an unhandled error
 * before the assertion attaches, which turns a passing expectation into a
 * confusing top-level failure. Catching explicitly is unambiguous, and it
 * also lets a test assert several things about the same error rather than
 * one matcher's worth.
 *
 * Fails loudly if the promise *resolves* - a test that meant to assert a
 * rejection must never quietly pass because nothing was thrown.
 */
export async function expectRejection(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("expected the promise to reject, but it resolved");
}
