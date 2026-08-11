Verify the migrated Cloudflare Queues web crawler example works at {{URL}}.

Clone the original source from https://github.com/cloudflare/queues-web-crawler/tree/7d9bd009881e26e852ae850d739a70837df0dd67 and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm a form UI loads for submitting URLs to crawl
2. Submit a URL (e.g., https://example.com) via the form
3. Confirm the URL is accepted and queued for processing
4. Wait briefly and check for crawl results (page title or link data)
5. Submit another URL and confirm it also gets processed

## Code review

Read through the migrated source and compare against the original. Check that:

- The queue producer pattern (`Queue.send`) is migrated to actor queue or message passing
- The queue consumer pattern (batch handler with `message.ack()` / `message.retry()`) is migrated to actor queue consumption (`c.queue.iter()` or equivalent)
- Message batching behavior is preserved
- The crawl results are stored (replacing Workers KV with actor state or SQLite)
- The HTML form for submitting URLs is preserved
- The producer/consumer separation is maintained (even if both run in actor context)
- No original functionality was silently dropped (aside from Browser Rendering replaced with fetch)

## Pass criteria

- Form UI loads without errors
- Can submit a URL for crawling
- URL gets queued and processed
- Crawl results are stored and retrievable
- Queue producer/consumer pattern is correctly implemented
- No original features are missing from the migrated code (except Browser Rendering replaced with fetch)
