mod test_messages;

pub use test_messages::*;

use std::time::Duration;

pub const CHANNEL_TEST_ITERATIONS: usize = 5000;
pub const CHANNEL_TEST_SENDERS: usize = 10;
pub const CHANNEL_TEST_RECEIVERS: usize = 10;
pub const TEST_TIMEOUT: Duration = Duration::from_millis(60000);
