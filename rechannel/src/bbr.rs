use std::time::{Duration, Instant};

// Length of the BBR.BtlBw max filter window for BBR.BtlBwFilter,
// BtlBwFilterLen is 10 packet-timed round trips.
const BTLBW_FILTER_LEN: Duration = Duration::from_secs(10);

// Length of the RTProp min filter window, RTpropFilterLen is 10 secs.
const RT_PROP_FILTER_LEN: Duration = Duration::from_secs(10);

// Minimum gain value that will allow the sending rate to double each round (2/ln(2) ~= 2.89),
// used in Startup mode for both BBR.pacing_gain and BBR.cwnd_gain.
const BBR_HIGH_GAIN: f64 = 2.89;

// Minimal cwnd value BBR tries to target using: 4 packets, or 4 * SMSS
const BBR_MIN_PIPE_CWND: usize = 4;

// Number of phases in the BBR ProbeBW gain cycle: 8.
const BBR_GAIN_CYCLE_LEN: usize = 8;

// Minimum time interval between ProbeRTT states: 10 secs.
const PROBE_RTT_INTERVAL: Duration = Duration::from_secs(10);

// Minimum duration for which ProbeRTT state holds inflight to BBRMinPipeCwnd or fewer packets: 200 ms.
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);

/// BBR Internal State Machine.
#[derive(Debug, PartialEq, Eq)]
enum BBRMode {
    Startup,
    Drain,
    ProbeBW,
    ProbeRTT,
}

struct BbrCongestionControl {
    // The current pacing rate for a BBR flow, which controls inter-packet spacing.
    pacing_rate: u64,
    // The transport sender's congestion window, which limits the amount of data in flight.
    cwnd: u64,
    // BBR's estimated bottleneck bandwidth available to the transport flow,
    // estimated from the maximum delivery rate sample in a sliding window.
    btl_bw: u64,
    // The max filter used to estimate BBR.BtlBw.
    btl_bw_filter: u64, // FIXME: not u64
    // BBR's estimated two-way round-trip propagation delay of the path,
    // estimated from the windowed minimum recent round-trip delay sample.
    rt_prop: Duration,
    // The wall clock time at which the current BBR.RTProp sample was obtained.
    rt_prop_stamp: Instant,
    // A boolean recording whether the BBR.RTprop has expired and is due
    // for a refresh with an application idle period or a transition into ProbeRTT state.
    rt_prop_expired: bool,
    // The dynamic gain factor used to scale BBR.BtlBw to produce BBR.pacing_rate.
    pacing_gain: f64,
    // The dynamic gain factor used to scale the estimated BDP to produce a congestion window (cwnd).
    cwnd_gain: f64,
    // Count of packet-timed round trips.
    round_count: u64,
    // A boolean that BBR sets to true once per packet-timed round trip, on ACKs that advance BBR.round_count.
    round_start: bool,
    // packet.delivered value denoting the end of a packet-timed round trip.
    next_round_delived: usize,
}

// Upon transport connection initialization, BBR executes its
// initialization steps:
//
// BBROnConnectionInit():
//    BBRInit()

// On every ACK, the BBR algorithm executes the following
// BBRUpdateOnACK() steps in order to update its network path model,
// update its state machine, and adjust its control parameters to adapt
// to the updated model:
//
// BBRUpdateOnACK():
//  BBRUpdateModelAndState()
//  BBRUpdateControlParameters()
//
// BBRUpdateModelAndState():
//  BBRUpdateBtlBw()
//  BBRCheckCyclePhase()
//  BBRCheckFullPipe()
//  BBRCheckDrain()
//  BBRUpdateRTprop()
//  BBRCheckProbeRTT()
//
// BBRUpdateControlParameters():
//  BBRSetPacingRate()
//  BBRSetSendQuantum()
//  BBRSetCwnd()

// When transmitting, BBR merely needs to check for the case where the
// flow is restarting from idle:
//
// BBROnTransmit():
//  BBRHandleRestartFromIdle()

struct DeliveryRateEstimator {
    // The total amount of data (measured in octets or in packets) delivered so far over the lifetime of the transport connection.
    // This does not include pure ACK packets.
    delivered_bytes: u64,
    // The wall clock time when delivered was last updated.
    delivered_time: Instant,
    // If packets are in flight, then this holds the send time of the packet that was most recently marked as delivered.
    // Else, if the connection was recently idle, then this holds the send time of most recently sent packet.
    first_sent_time: Instant,
    // The index of the last transmitted packet marked as application-limited,
    // or 0 if the connection is not currently application-limited.
    app_limited: u64,
    rate_sample: RateSample,
}

struct PacketSentInfo {
    // C.delivered When the packet was sent from transport connection C.
    delivered: u64,
    // C.delivered_time when the packet was sent
    delivered_time: Instant,
    // C.first_sent_time when the packet was sent.
    first_sent_time: Instant,
    // true if C.app_limited was non-zero when the packet was sent, else false.
    is_app_limited: bool,
    // The time when the packet was sent
    sent_time: Instant,
    // Size of the packet
    data_length: usize,
}

#[derive(Default, Debug)]
struct RateSample {
    // The delivery rate sample (in most cases rs.delivered / rs.interval).
    delivery_rate: u64,
    // The P.is_app_limited from the most recent packet delivered; indicates whether the rate sample is application-limited.
    is_app_limited: bool,
    // The length of the sampling interval
    interval: Duration,
    // The amount of data marked as delivered over the sampling interval.
    delivered: u64,
    // The P.delivered count from the most recent packet delivered.
    prior_delivered: u64,
    // The P.delivered_time from the most recent packet delivered.
    prior_time: Option<Instant>,
    // Send time interval calculated from the most recent packet delivered.
    send_elapsed: Duration,
    // ACK time interval calculated from the most recent packet delivered
    ack_elapsed: Duration,
}

impl DeliveryRateEstimator {
    // if (SND.NXT == SND.UNA)  /* no packets in flight yet? */
    //   C.first_sent_time  = C.delivered_time = Now()
    // P.first_sent_time = C.first_sent_time
    // P.delivered_time  = C.delivered_time
    // P.delivered       = C.delivered
    // P.is_app_limited  = (C.app_limited != 0)
    fn on_packet_sent(&mut self, packet_sent: &mut PacketSentInfo, bytes_in_flight: u64) {
        // No packets in flight yet
        if bytes_in_flight == 0 {
            self.first_sent_time = packet_sent.sent_time;
            self.delivered_time = packet_sent.sent_time;
        }

        packet_sent.first_sent_time = self.first_sent_time;
        packet_sent.delivered_time = self.delivered_time;
        packet_sent.delivered = self.delivered_bytes;
        packet_sent.is_app_limited = self.app_limited != 0;
    }

    fn on_packet_ack(&mut self, acked: &mut PacketSentInfo, now: Instant) {
        self.delivered_bytes += acked.data_length as u64;
        self.delivered_time = now;

        if self.rate_sample.prior_time.is_none() || acked.delivered > self.rate_sample.prior_delivered {
            self.rate_sample.prior_delivered = acked.delivered;
            self.rate_sample.prior_time = Some(acked.delivered_time);
            self.rate_sample.is_app_limited = acked.is_app_limited;
            self.rate_sample.send_elapsed = acked.sent_time - self.first_sent_time;
            self.rate_sample.ack_elapsed = self.delivered_time - acked.delivered_time;
            self.first_sent_time = acked.sent_time;
        }
    }

    fn generate_rate_sample(&mut self, min_rtt: Duration) {
        if self.app_limited != 0 && self.delivered_bytes > self.app_limited {
            self.app_limited = 0;
        }

        if self.rate_sample.prior_time.is_none() {
            return;
        }

        self.rate_sample.interval = self.rate_sample.send_elapsed.max(self.rate_sample.ack_elapsed);
        self.rate_sample.delivered = self.delivered_bytes - self.rate_sample.prior_delivered;

        if self.rate_sample.interval < min_rtt {
            self.rate_sample.interval = Duration::ZERO;

            return;
        }

        if !self.rate_sample.interval.is_zero() {
            self.rate_sample.delivery_rate = (self.rate_sample.delivered as f64 / self.rate_sample.interval.as_secs_f64()) as u64;
        }
    }
}

// V2

struct BandwidthEstimation {
    total_acked: u64,
    prev_total_acked: u64,
    acked_time: Option<Instant>,
    prev_acked_time: Option<Instant>,
    total_sent: u64,
    prev_total_sent: u64,
    sent_time: Instant,
    prev_sent_time: Option<Instant>,
    max_filter: MinMax,
    acked_at_last_window: u64,
}

impl BandwidthEstimation {
    pub fn on_sent(&mut self, now: Instant, bytes: u64) {
        self.prev_total_sent = self.total_sent;
        self.total_sent += bytes;
        self.prev_sent_time = Some(self.sent_time);
        self.sent_time = now;
    }

    pub fn on_ack(&mut self, now: Instant, bytes: u64, app_limited: bool) {
        self.prev_total_acked = self.total_acked;
        self.total_acked += bytes;
        self.prev_acked_time = self.acked_time;
        self.acked_time = Some(now);

        let Some(prev_sent_time) = self.prev_sent_time else {
            return;
        };

        let send_rate = if self.sent_time > prev_sent_time {
            bandwidth_from_delta(self.total_sent - self.prev_total_sent, self.sent_time - prev_sent_time)
        } else {
            u64::MAX // will take the min of send and ack, so this is just a skip
        };

        let ack_rate = match self.prev_acked_time {
            Some(prev_acked_time) => bandwidth_from_delta(self.total_acked - self.prev_total_acked, now - prev_acked_time),
            None => 0,
        };

        let bandwidth = send_rate.min(ack_rate);
        if !app_limited && self.max_filter.get() < bandwidth {
            self.max_filter.update_max(BTLBW_FILTER_LEN, now, bandwidth);
        }
    }

    pub fn bytes_acked_this_window(&self) -> u64 {
        self.total_acked - self.acked_at_last_window
    }

    pub fn end_acks(&mut self, _current_round: u64, _app_limited: bool) {
        self.acked_at_last_window = self.total_acked;
    }

    pub fn get_estimate(&self) -> u64 {
        self.max_filter.get()
    }
}

pub const fn bandwidth_from_delta(bytes: u64, delta: Duration) -> u64 {
    let window_duration_ns = delta.as_nanos();
    if window_duration_ns == 0 {
        return 0;
    }
    let b_ns = bytes * 1_000_000_000;
    let bytes_per_second = b_ns / (window_duration_ns as u64);
    bytes_per_second
}

// Kathleen Nichols' algorithm for tracking the minimum (or maximum)
// value of a data stream over some fixed time interval.  (E.g.,
// the minimum RTT over the past five minutes.) It uses constant
// space and constant time per update yet almost always delivers
// the same minimum as an implementation that has to keep all the
// data in the window.
//
// The algorithm keeps track of the best, 2nd best & 3rd best min
// values, maintaining an invariant that the measurement time of
// the n'th best >= n-1'th best. It also makes sure that the three
// values are widely separated in the time window since that bounds
// the worse case error when that data is monotonically increasing
// over the window.
//
// Upon getting a new min, we can forget everything earlier because
// it has no value - the new min is <= everything else in the window
// by definition and it's the most recent. So we restart fresh on
// every new min and overwrites 2nd & 3rd choices. The same property
// holds for 2nd & 3rd best.

#[derive(Debug, Copy, Clone)]
struct MinMaxSample {
    time: Instant,
    value: u64,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MinMax {
    samples: [MinMaxSample; 3],
}

impl MinMax {
    pub fn new() -> Self {
        MinMax {
            samples: [MinMaxSample {
                value: 0,
                time: Instant::now(),
            }; 3],
        }
    }

    pub fn get(&self) -> u64 {
        self.samples[0].value
    }

    pub fn fill(&mut self, value: u64, time: Instant) {
        self.samples.fill(MinMaxSample { time, value });
    }

    pub fn update_max(&mut self, window: Duration, time: Instant, value: u64) {
        let sample = MinMaxSample { time, value };

        let dt = time.duration_since(self.samples[2].time);

        // Fill samples if found new max or delta time is above the window
        if sample.value >= self.samples[0].value || dt > window {
            self.fill(value, time);
            return;
        }

        if sample.value >= self.samples[1].value {
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if sample.value <= self.samples[2].value {
            self.samples[2] = sample;
        }

        self.subwin_update(window, sample);
    }

    // As time advances, update the 1st, 2nd, and 3rd choices.
    fn subwin_update(&mut self, window: Duration, sample: MinMaxSample) {
        let dt = sample.time - self.samples[0].time;
        if dt > window {
            // Passed entire window without a new sample so make 2nd
            // choice the new sample & 3rd choice the new 2nd choice.
            // we may have to iterate this since our 2nd choice
            // may also be outside the window (we checked on entry
            // that the third choice was in the window).
            self.samples[0] = self.samples[1];
            self.samples[1] = self.samples[2];
            self.samples[2] = sample;
            if sample.time - self.samples[0].time > window {
                self.samples[0] = self.samples[1];
                self.samples[1] = self.samples[2];
                self.samples[2] = sample;
            }
        } else if self.samples[1].time == self.samples[0].time && dt > window / 4 {
            // We've passed a quarter of the window without a new sample
            // so take a 2nd choice from the 2nd quarter of the window.
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if self.samples[2].time == self.samples[1].time && dt > window / 2 {
            // We've passed half the window without finding a new sample
            // so take a 3rd choice from the last half of the window
            self.samples[2] = sample;
        }
    }
}
