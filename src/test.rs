mod test_messages;

pub use test_messages::*;

use std::time::Duration;

pub const CHANNEL_TEST_ITERATIONS: usize = 2000;
pub const CHANNEL_TEST_SENDERS: usize = 10;
pub const CHANNEL_TEST_RECEIVERS: usize = 5;
pub const TEST_TIMEOUT: Duration = Duration::from_secs(100);
