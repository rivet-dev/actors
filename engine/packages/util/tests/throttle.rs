use rivet_util::throttle::{RateLimitMethod, RateLimiter};
use tokio::time::{Duration, Instant};

// MARK: FixedWindow / try_acquire

#[tokio::test(start_paused = true)]
async fn fixed_window_allows_full_burst_then_blocks() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 3,
		period: Duration::from_millis(100),
	});

	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	// Limit reached within the window.
	assert!(!rl.try_acquire());
}

#[tokio::test(start_paused = true)]
async fn fixed_window_does_not_refill_before_period() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 2,
		period: Duration::from_millis(100),
	});

	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());

	// Just shy of a full period: still no refill. The window is
	// all-or-nothing, it does not drip partial credit.
	tokio::time::advance(Duration::from_millis(99)).await;
	assert!(!rl.try_acquire());
}

#[tokio::test(start_paused = true)]
async fn fixed_window_resets_to_full_after_period() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 2,
		period: Duration::from_millis(100),
	});

	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());

	// After a full period the window resets to its full allowance.
	tokio::time::advance(Duration::from_millis(100)).await;
	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}

// MARK: LeakyBucket / try_acquire

#[tokio::test(start_paused = true)]
async fn leaky_bucket_allows_full_burst_then_blocks() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 3,
		drip_rate: Duration::from_millis(10),
	});

	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}

#[tokio::test(start_paused = true)]
async fn leaky_bucket_drips_exactly_one_token_per_rate() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 3,
		drip_rate: Duration::from_millis(10),
	});

	// Drain the bucket.
	for _ in 0..3 {
		assert!(rl.try_acquire());
	}
	assert!(!rl.try_acquire());

	// Exactly one drip period yields exactly one token, no more.
	tokio::time::advance(Duration::from_millis(10)).await;
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}

#[tokio::test(start_paused = true)]
async fn leaky_bucket_refill_is_capped_at_capacity() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 3,
		drip_rate: Duration::from_millis(10),
	});

	for _ in 0..3 {
		assert!(rl.try_acquire());
	}
	assert!(!rl.try_acquire());

	// Idle far longer than it takes to refill the whole bucket. Credit must
	// not accumulate past capacity, so only `requests` tokens are available.
	tokio::time::advance(Duration::from_millis(1_000)).await;
	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}

#[tokio::test(start_paused = true)]
async fn leaky_bucket_accumulates_fractional_drip_across_calls() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 1,
		drip_rate: Duration::from_millis(10),
	});

	// Consume the only token.
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());

	// Half a drip period: less than one whole token, still blocked.
	tokio::time::advance(Duration::from_millis(5)).await;
	assert!(!rl.try_acquire());

	// Another half period: the fractional credit from the previous interval
	// must carry over and complete one whole token.
	tokio::time::advance(Duration::from_millis(5)).await;
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}

// MARK: acquire (blocking)

#[tokio::test(start_paused = true)]
async fn acquire_returns_immediately_while_tokens_remain() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 3,
		drip_rate: Duration::from_millis(10),
	});

	let start = Instant::now();
	rl.acquire().await;
	rl.acquire().await;
	rl.acquire().await;
	// Burst is served without waiting.
	assert_eq!(start.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn acquire_blocks_until_a_token_is_available() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 1,
		drip_rate: Duration::from_millis(10),
	});

	// Drain the single token.
	rl.acquire().await;

	// The next acquire must wait one full drip period for a token.
	let start = Instant::now();
	rl.acquire().await;
	assert!(start.elapsed() >= Duration::from_millis(10));
}

#[tokio::test(start_paused = true)]
async fn acquire_sustains_the_drip_rate_without_doubling() {
	let mut rl = RateLimiter::new(RateLimitMethod::LeakyBucket {
		requests: 1,
		drip_rate: Duration::from_millis(10),
	});

	// Drain the initial burst token so every subsequent acquire starts empty.
	rl.acquire().await;

	let start = Instant::now();
	// Five acquires, each starting from an empty bucket, must each cost one
	// drip period, so the total is at least 5 * drip_rate. A limiter that
	// admits the post-sleep request without debiting a token finishes in
	// ~3 periods, effectively doubling the sustained rate.
	for _ in 0..5 {
		rl.acquire().await;
	}
	assert!(start.elapsed() >= Duration::from_millis(50));
}

#[tokio::test(start_paused = true)]
async fn fixed_window_acquire_blocks_until_window_resets() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 2,
		period: Duration::from_millis(100),
	});

	rl.acquire().await;
	rl.acquire().await;

	// The window is exhausted, so the next acquire must wait for the reset.
	let start = Instant::now();
	rl.acquire().await;
	assert!(start.elapsed() >= Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn fixed_window_acquire_waits_only_for_the_remainder_of_the_window() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 1,
		period: Duration::from_millis(100),
	});

	rl.acquire().await;

	// Most of the window has already elapsed, so only the remainder is left to wait. Sleeping a
	// whole period from now would stall the caller for 100ms instead of 40ms.
	tokio::time::advance(Duration::from_millis(60)).await;
	let start = Instant::now();
	rl.acquire().await;
	assert_eq!(start.elapsed(), Duration::from_millis(40));
}

#[tokio::test(start_paused = true)]
async fn fixed_window_acquire_debits_the_window_it_waited_for() {
	let mut rl = RateLimiter::new(RateLimitMethod::FixedWindow {
		requests: 2,
		period: Duration::from_millis(100),
	});

	rl.acquire().await;
	rl.acquire().await;

	// This blocks until the window resets and must take its token out of the refilled window. A
	// limiter that hands the caller a full window without debiting it admits `limit + 1`.
	rl.acquire().await;
	assert!(rl.try_acquire());
	assert!(!rl.try_acquire());
}
