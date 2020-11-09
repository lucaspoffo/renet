use crate::SequenceBuffer;
use bincode;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

trait Content: serde::ser::Serialize + serde::de::DeserializeOwned + Default + Clone {}
impl<T: serde::ser::Serialize + serde::de::DeserializeOwned + Default + Clone> Content for T {}

struct ChannelConfig {
    sent_packet_buffer_size: usize,
    message_send_queue_size: usize,
    message_receive_queue_size: usize,
    max_message_per_packet: u32,
    packet_budget: Option<u32>,
    message_resend_time: Duration,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            sent_packet_buffer_size: 1024,
            message_send_queue_size: 1024,
            message_receive_queue_size: 1024,
            max_message_per_packet: 256,
            packet_budget: None,
            message_resend_time: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message<T> {
    id: u16,
    content: T,
}

impl<T> Message<T> {
    fn new(id: u16, content: T) -> Self {
        Self { id, content }
    }
}

impl<T: Default> Default for Message<T> {
    fn default() -> Self {
        Self {
            id: 0,
            content: T::default(),
        }
    }
}

trait Channel<T> {
    fn get_packet_data(
        &mut self,
        available_bits: u32,
        sequence: u16,
    ) -> Option<ChannelPacketData<T>>;
    fn process_packet_data(&mut self, packet_data: ChannelPacketData<T>);
    fn process_ack(&mut self, ack: u16);
    fn send_message(&mut self, message: T);
    fn identifier(&self) -> u8;
    fn receive_message(&mut self) -> Option<Message<T>>;
    fn reset(&mut self);
}

#[derive(Debug, Clone)]
struct MessageSend<T> {
    message: Message<T>,
    last_time_sent: Option<Instant>,
    serialized_size_bits: u32,
}

impl<T: Content> MessageSend<T> {
    fn new(message: Message<T>) -> Self {
        Self {
            serialized_size_bits: bincode::serialized_size(&message).unwrap() as u32 * 8,
            message,
            last_time_sent: None,
        }
    }
}

impl<T: Default> Default for MessageSend<T> {
    fn default() -> Self {
        Self {
            message: Message::default(),
            last_time_sent: None,
            serialized_size_bits: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct PacketSent {
    acked: bool,
    time_sent: Instant,
    messages_id: Vec<u16>,
}

impl PacketSent {
    fn new(messages_id: Vec<u16>) -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id,
        }
    }
}

impl Default for PacketSent {
    fn default() -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id: vec![],
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ChannelPacketData<T> {
    messages: Vec<Message<T>>,
    channel_index: u8,
}

impl<T> ChannelPacketData<T> {
    fn new(messages: Vec<Message<T>>, channel_index: u8) -> Self {
        Self {
            messages,
            channel_index,
        }
    }
}

struct ReliableOrderedChannel<T> {
    config: ChannelConfig,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_send: SequenceBuffer<MessageSend<T>>,
    messages_received: SequenceBuffer<Message<T>>,
    send_message_id: u16,
    received_message_id: u16,
    num_messages_sent: u64,
    num_messages_received: u64,
    oldest_unacked_message_id: u16,
}

impl<T: Content> ReliableOrderedChannel<T> {
    fn new(config: ChannelConfig) -> Self {
        Self {
            packets_sent: SequenceBuffer::with_capacity(config.sent_packet_buffer_size),
            messages_send: SequenceBuffer::with_capacity(config.message_send_queue_size),
            messages_received: SequenceBuffer::with_capacity(config.message_receive_queue_size),
            send_message_id: 0,
            received_message_id: 0,
            num_messages_received: 0,
            num_messages_sent: 0,
            oldest_unacked_message_id: 0,
            config,
        }
    }

    fn has_messages_to_send(&self) -> bool {
        self.oldest_unacked_message_id != self.send_message_id
    }

    // TODO: use bits or bytes?
    fn get_messages_to_send(&mut self, available_bits: u32) -> Option<Vec<u16>> {
        if !self.has_messages_to_send() {
            return None;
        }

        let mut available_bits = if let Some(packet_budget) = self.config.packet_budget {
            std::cmp::min(packet_budget * 8, available_bits)
        } else {
            available_bits
        };

        let message_limit = std::cmp::min(
            self.config.message_send_queue_size,
            self.config.message_receive_queue_size,
        );
        // TODO: set current time in channel field?
        let now = Instant::now();
        let mut num_messages = 0;
        let mut messages_id = vec![];

        for i in 0..message_limit {
            if num_messages == self.config.max_message_per_packet {
                break;
            }
            let message_id = self.oldest_unacked_message_id + i as u16;
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                let send = if let Some(last_time_sent) = message_send.last_time_sent {
                    (last_time_sent + self.config.message_resend_time) <= now
                } else {
                    true
                };

                if send && message_send.serialized_size_bits <= available_bits {
                    messages_id.push(message_id);
                    num_messages += 1;
                    available_bits -= message_send.serialized_size_bits;
                }
            }
        }

        if messages_id.len() > 0 {
            return Some(messages_id);
        }
        None
    }

    fn get_messages_packet_data(&self, messages_id: &[u16]) -> ChannelPacketData<T> {
        let mut messages: Vec<Message<T>> = Vec::with_capacity(messages_id.len());
        for &message_id in messages_id.iter() {
            let message_send = self
                .messages_send
                .get(message_id)
                .expect("Invalid message id when generating packet data");
            // TODO: can we remove this clone? and pass the reference
            messages.push(message_send.message.clone());
        }
        ChannelPacketData::new(messages, self.identifier())
    }

    fn add_messages_packet_entry(&mut self, messages_id: Vec<u16>, sequence: u16) {
        let packet_sent = PacketSent::new(messages_id);
        self.packets_sent.insert(sequence, packet_sent);
    }

    fn update_oldest_message_ack(&mut self) {
        let stop_id = self.messages_send.sequence();

        while self.oldest_unacked_message_id != stop_id
            && !self.messages_send.exists(self.oldest_unacked_message_id)
        {
            self.oldest_unacked_message_id += 1;
        }
    }
}

impl<T: Content> Channel<T> for ReliableOrderedChannel<T> {
    fn get_packet_data(
        &mut self,
        available_bits: u32,
        sequence: u16,
    ) -> Option<ChannelPacketData<T>> {
        if let Some(messages_id) = self.get_messages_to_send(available_bits) {
            let data = self.get_messages_packet_data(&messages_id);
            self.add_messages_packet_entry(messages_id, sequence);
            return Some(data);
        }
        None
    }

    fn process_packet_data(&mut self, packet_data: ChannelPacketData<T>) {
        for message in packet_data.messages.iter() {
            // TODO: validate min max message_id based on config queue size
            let message_id = message.id;
            if !self.messages_received.exists(message_id) {
                self.messages_received.insert(message_id, message.clone());
            }
        }
    }

    fn process_ack(&mut self, ack: u16) {
        if let Some(sent_packet) = self.packets_sent.get_mut(ack) {
            // Should we assert already acked?
            if sent_packet.acked {
                return;
            }

            for &message_id in sent_packet.messages_id.iter() {
                if self.messages_send.exists(message_id) {
                    self.messages_send.remove(message_id);
                }
            }
            self.update_oldest_message_ack();
        }
    }

    fn send_message(&mut self, message: T) {
        // assert that can send message?
        // Check config for max num size
        let message_id = self.send_message_id;
        self.send_message_id = self.send_message_id.wrapping_add(1);

        let entry = MessageSend::new(Message::new(message_id, message));
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;
    }

    fn identifier(&self) -> u8 {
        0
    }

    fn receive_message(&mut self) -> Option<Message<T>> {
        let received_message_id = self.received_message_id;

        if !self.messages_received.exists(received_message_id) {
            return None;
        }

        self.received_message_id = self.received_message_id.wrapping_add(1);
        self.num_messages_received += 1;

        return self.messages_received.remove(received_message_id);
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Serialize, Deserialize}; 
    #[derive(Debug, Serialize, Deserialize, Clone)]
    enum TestMessages {
        Noop,
        First,
        Second(u32)
    }

    impl Default for TestMessages {
        fn default() -> Self {
            return TestMessages::Noop
        }
    }

    #[test]
    fn send_message() {
        let config = ChannelConfig::default();
        let mut channel: ReliableOrderedChannel<TestMessages> = ReliableOrderedChannel::new(config);

        channel.send_message(TestMessages::Second(0));
        assert!(channel.receive_message().is_none());

    }
}
