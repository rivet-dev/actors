use rand::Rng;
use tokio::time::{Duration, Instant};

pub struct Backoff {
	/// Maximum exponent for the backoff.
	max_exponent: usize,

	/// Maximum amount of retries.
	max_retries: Option<usize>,

	/// Base wait time in ms.
	wait: usize,

	/// Maximum randomness.
	randomness: usize,

	/// Iteration of the backoff.
	i: usize,

	/// Timestamp to sleep until in ms.
	sleep_until: Instant,
}

impl Backoff {
	pub fn new(
		max_exponent: usize,
		max_retries: Option<usize>,
		wait: usize,
		randomness: usize,
	) -> Backoff {
		Backoff {
			max_exponent,
			max_retries,
			wait,
			randomness,
			i: 0,
			sleep_until: Instant::now(),
		}
	}

	pub fn new_at(
		max_exponent: usize,
		max_retries: Option<usize>,
		wait: usize,
		randomness: usize,
		i: usize,
	) -> Backoff {
		Backoff {
			max_exponent,
			max_retries,
			wait,
			randomness,
			i,
			sleep_until: Instant::now(),
		}
	}

	pub fn tick_index(&self) -> usize {
		self.i
	}

	/// Waits for the next backoff tick.
	///
	/// Returns false if the index is greater than `max_retries`.
	pub async fn tick(&mut self) -> bool {
		if self.max_retries.map_or(false, |x| self.i > x) {
			return false;
		}

		tokio::time::sleep_until(self.sleep_until).await;

		let next_wait = self.current_duration() + rand::thread_rng().gen_range(0..self.randomness);
		self.sleep_until += Duration::from_millis(next_wait as u64);

		self.i += 1;

		true
	}

	/// Returns the instant of the next backoff tick. Does not wait.
	///
	/// Returns None if the index is greater than `max_retries`.
	pub fn step(&mut self) -> Option<Instant> {
		if self.max_retries.map_or(false, |x| self.i > x) {
			return None;
		}

		let next_wait = self.current_duration() + rand::thread_rng().gen_range(0..self.randomness);
		self.sleep_until += Duration::from_millis(next_wait as u64);

		self.i += 1;

		Some(self.sleep_until)
	}

	pub fn current_duration(&self) -> usize {
		self.wait * 2usize.pow(self.i.min(self.max_exponent) as u32)
	}

	pub fn default_infinite() -> Backoff {
		Backoff::new(8, None, 1_000, 1_000)
	}
}

impl Default for Backoff {
	fn default() -> Backoff {
		Backoff::new(5, Some(16), 1_000, 1_000)
	}
}

pub enum RateLimitMethod {
	FixedWindow {
		requests: u64,
		period: Duration,
	},
	LeakyBucket {
		requests: u64,
		/// How quickly to regain requests. 1 / drip_rate
		drip_rate: Duration,
	},
}

enum RateLimitState {
	FixedWindow {
		requests_remaining: u64,
		requests_limit: u64,
		reset_time: Instant,
		period: Duration,
	},
	LeakyBucket {
		requests_remaining: u64,
		requests_limit: u64,
		last_acquire: Instant,
		drip_rate: Duration,
		accum_drip: f32,
	},
}

pub struct RateLimiter {
	state: RateLimitState,
}

impl RateLimiter {
	pub fn new(method: RateLimitMethod) -> Self {
		Self {
			state: match method {
				RateLimitMethod::FixedWindow { requests, period } => RateLimitState::FixedWindow {
					requests_remaining: requests,
					requests_limit: requests,
					reset_time: Instant::now() + period,
					period,
				},
				RateLimitMethod::LeakyBucket {
					requests,
					drip_rate,
				} => RateLimitState::LeakyBucket {
					requests_remaining: requests,
					requests_limit: requests,
					last_acquire: Instant::now(),
					drip_rate: drip_rate,
					accum_drip: 0.0,
				},
			},
		}
	}

	pub fn try_acquire(&mut self) -> bool {
		match &mut self.state {
			RateLimitState::FixedWindow {
				requests_remaining,
				requests_limit,
				reset_time,
				period,
			} => {
				let now = Instant::now();
				// Check if we need to reset the counter
				if now >= *reset_time {
					*requests_remaining = *requests_limit;
					*reset_time = now + *period;
				}

				// Try to consume a request
				if *requests_remaining > 0 {
					*requests_remaining -= 1;
					true
				} else {
					false
				}
			}
			RateLimitState::LeakyBucket {
				requests_remaining,
				requests_limit,
				last_acquire,
				drip_rate,
				accum_drip,
			} => {
				let now = Instant::now();
				let dt = now - *last_acquire;
				*last_acquire = now;

				// Drip bucket
				if requests_remaining < requests_limit {
					*accum_drip += dt.div_duration_f32(*drip_rate);

					*requests_remaining +=
						(*accum_drip as u64).min(*requests_limit - *requests_remaining);

					if *accum_drip >= 1.0 {
						*accum_drip = accum_drip.fract();
					}
				}

				if *requests_remaining > 0 {
					*requests_remaining -= 1;
					true
				} else {
					false
				}
			}
		}
	}

	pub async fn acquire(&mut self) {
		match &mut self.state {
			RateLimitState::FixedWindow {
				requests_remaining,
				requests_limit,
				reset_time,
				period,
			} => {
				let now = Instant::now();
				// Check if we need to reset the counter
				if now >= *reset_time {
					*requests_remaining = *requests_limit;
					*reset_time = now + *period;
				}

				// Try to consume a request
				if *requests_remaining > 0 {
					*requests_remaining -= 1;
				} else {
					// Wait for the current window to reset rather than a full period from now,
					// then debit this caller's token from the refilled window.
					tokio::time::sleep_until(*reset_time).await;

					*requests_remaining = requests_limit.saturating_sub(1);
					*reset_time = Instant::now() + *period;
				}
			}
			RateLimitState::LeakyBucket {
				requests_remaining,
				requests_limit,
				last_acquire,
				drip_rate,
				accum_drip,
			} => {
				let now = Instant::now();
				let dt = now - *last_acquire;
				*last_acquire = now;

				// Drip bucket
				if requests_remaining < requests_limit {
					*accum_drip += dt.div_duration_f32(*drip_rate);

					*requests_remaining +=
						(*accum_drip as u64).min(*requests_limit - *requests_remaining);

					if *accum_drip >= 1.0 {
						*accum_drip = accum_drip.fract();
					}
				}

				if *requests_remaining > 0 {
					*requests_remaining -= 1;
				} else {
					let deficit = 1.0 - *accum_drip;
					tokio::time::sleep(drip_rate.mul_f32(deficit)).await;

					*last_acquire = Instant::now();
					*accum_drip = 0.0;
				}
			}
		}
	}
}
