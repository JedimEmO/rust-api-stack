//! Bounded queue for messages a transport has received but the client has not yet handled.
//!
//! The browser WebSocket API delivers every message as an event and has no way to push back on
//! the server, so the WASM transport queues them itself. The queue is bounded like the native
//! transport, which reads one message at a time and leaves the rest to TCP flow control: a
//! message over [`MAX_INBOUND_MESSAGE_SIZE`], more than `max_messages` queued, or more than
//! [`MAX_QUEUED_BYTES`] queued is an overflow, and the transport closes the connection.

use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
use std::collections::VecDeque;

/// Largest message either transport accepts, in bytes.
pub(crate) const MAX_INBOUND_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Most bytes of unhandled messages the queue holds at once.
pub(crate) const MAX_QUEUED_BYTES: usize = 4 * MAX_INBOUND_MESSAGE_SIZE;

/// Why the queue refused a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundOverflow {
    /// One message is over [`MAX_INBOUND_MESSAGE_SIZE`].
    MessageTooLarge,
    /// `max_messages` are already queued.
    TooManyMessages,
    /// The queued messages would exceed [`MAX_QUEUED_BYTES`].
    TooManyBytes,
}

impl std::fmt::Display for InboundOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MessageTooLarge => "inbound message too large",
            Self::TooManyMessages => "too many unhandled inbound messages",
            Self::TooManyBytes => "too many bytes of unhandled inbound messages",
        })
    }
}

#[derive(Debug)]
pub(crate) struct InboundQueue {
    messages: VecDeque<(BidirectionalMessage, usize)>,
    bytes: usize,
    max_messages: usize,
}

impl InboundQueue {
    pub(crate) fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            bytes: 0,
            max_messages: max_messages.max(1),
        }
    }

    /// Refuse a message by its size before it is parsed.
    pub(crate) fn check_size(size: usize) -> Result<(), InboundOverflow> {
        if size > MAX_INBOUND_MESSAGE_SIZE {
            Err(InboundOverflow::MessageTooLarge)
        } else {
            Ok(())
        }
    }

    /// Queue a parsed message that was `size` bytes on the wire.
    pub(crate) fn push(
        &mut self,
        message: BidirectionalMessage,
        size: usize,
    ) -> Result<(), InboundOverflow> {
        Self::check_size(size)?;
        if self.messages.len() >= self.max_messages {
            return Err(InboundOverflow::TooManyMessages);
        }
        if self.bytes + size > MAX_QUEUED_BYTES {
            return Err(InboundOverflow::TooManyBytes);
        }
        self.bytes += size;
        self.messages.push_back((message, size));
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<BidirectionalMessage> {
        let (message, size) = self.messages.pop_front()?;
        self.bytes -= size;
        Some(message)
    }

    pub(crate) fn clear(&mut self) {
        self.messages.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_refuses_messages_past_the_count_limit() {
        let mut queue = InboundQueue::new(2);
        queue.push(BidirectionalMessage::Ping, 10).unwrap();
        queue.push(BidirectionalMessage::Pong, 10).unwrap();
        assert_eq!(
            queue.push(BidirectionalMessage::Ping, 10),
            Err(InboundOverflow::TooManyMessages)
        );

        // Handling a message makes room again, in order.
        assert!(matches!(queue.pop(), Some(BidirectionalMessage::Ping)));
        queue.push(BidirectionalMessage::Ping, 10).unwrap();
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn queue_refuses_oversized_messages() {
        let mut queue = InboundQueue::new(8);
        assert_eq!(
            InboundQueue::check_size(MAX_INBOUND_MESSAGE_SIZE + 1),
            Err(InboundOverflow::MessageTooLarge)
        );
        assert_eq!(
            queue.push(BidirectionalMessage::Ping, MAX_INBOUND_MESSAGE_SIZE + 1),
            Err(InboundOverflow::MessageTooLarge)
        );
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn queue_refuses_messages_past_the_byte_limit() {
        let mut queue = InboundQueue::new(1024);
        for _ in 0..MAX_QUEUED_BYTES / MAX_INBOUND_MESSAGE_SIZE {
            queue
                .push(BidirectionalMessage::Ping, MAX_INBOUND_MESSAGE_SIZE)
                .unwrap();
        }
        assert_eq!(
            queue.push(BidirectionalMessage::Ping, 1),
            Err(InboundOverflow::TooManyBytes)
        );

        queue.pop();
        queue.push(BidirectionalMessage::Ping, 1).unwrap();
        queue.clear();
        assert_eq!(queue.len(), 0);
        queue
            .push(BidirectionalMessage::Ping, MAX_INBOUND_MESSAGE_SIZE)
            .unwrap();
    }
}
