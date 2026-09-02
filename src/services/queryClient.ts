import { QueryClient } from "@tanstack/react-query";

/**
 * The shared query cache, tuned for a client whose every answer costs an SSH
 * round trip.
 *
 * The defaults in this library are written for HTTP against a nearby API,
 * where a retry is cheap and a refetch is free. Here a query is a command
 * run on somebody's VPS: it opens a channel, waits for a shell, and comes
 * back a few hundred milliseconds later at best. Three of the defaults are
 * wrong for that, and they are the three set below.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      /**
       * How long a section's data is reused without asking again.
       *
       * This is the whole point of adding the library: switching to a tab
       * and back used to unmount it and refetch from zero, so a section you
       * looked at five seconds ago still showed a skeleton while a Node was
       * asked the same question again. Fifteen seconds is short enough that
       * nothing on screen is meaningfully out of date and long enough to
       * cover moving around the app.
       */
      staleTime: 15_000,

      /**
       * How long an unused answer is kept before being dropped.
       *
       * Longer than `staleTime` on purpose: a cached-but-stale answer is
       * what lets a section paint immediately and update a moment later,
       * which is the difference between "slow" and "fast" as far as anybody
       * looking at it is concerned.
       */
      gcTime: 5 * 60_000,

      /**
       * No automatic retries.
       *
       * The library's default is three, with backoff. Over SSH a failure is
       * usually a real answer - the Node is unreachable, the container is
       * gone, sudo was refused - and retrying it means the user waits
       * several extra seconds to be told something the first attempt
       * already knew. The polls that exist elsewhere will try again anyway.
       */
      retry: false,

      /**
       * Not on window focus.
       *
       * Alt-tabbing back would otherwise fire every mounted query at once,
       * which on a dashboard with several Nodes is a burst of SSH channels
       * for readings that are usually still fine. What needs to stay live
       * says so itself with `refetchInterval`, and `usePolling` already
       * resumes its own work when the window comes back.
       */
      refetchOnWindowFocus: false,
    },
  },
});
