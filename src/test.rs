pub const CHANNEL_TEST_ITERATIONS: usize = 1_000_000;
pub const CHANNEL_TEST_SENDERS: usize = 10;
pub const CHANNEL_TEST_RECEIVERS: usize = 10;

pub struct MessageIter<I> {
    sender: usize,
    iter: I,
}

impl<I> Iterator for MessageIter<I>
where
    I: Iterator<Item = usize>,
{
    type Item = Message;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|index| Message {
            sender: self.sender,
            index,
        })
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Message {
    sender: usize,
    index: usize,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            sender: 0,
            index: 0,
        }
    }
}

impl Message {
    pub fn new_iter(sender: usize) -> impl Iterator<Item = Message> {
        MessageIter {
            sender,
            iter: (0..CHANNEL_TEST_ITERATIONS),
        }
    }
}

pub struct Channels {
    channels: Vec<Channel>,
}

impl Channels {
    pub fn new(senders: usize) -> Self {
        let mut channels = Vec::with_capacity(senders);
        for i in 0..senders {
            channels.push(Channel::new(i));
        }
        Self { channels }
    }

    #[track_caller]
    pub fn assert_message(&mut self, message: &Message) {
        self.channels[message.sender].assert_message(message);
    }
}

pub struct Channel {
    sender: usize,
    current_index: usize,
}

impl Channel {
    pub fn new(sender: usize) -> Self {
        Self {
            sender,
            current_index: 0,
        }
    }

    #[track_caller]
    pub fn assert_message(&mut self, message: &Message) {
        assert_eq!(self.sender, message.sender);
        assert_eq!(self.current_index, message.index);
        self.current_index += 1;
    }
}
