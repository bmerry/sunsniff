/* Copyright 2022 Bruce Merry
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
 * more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program. If not, see <https://www.gnu.org/licenses/>.
 */

use chrono::{DateTime, LocalResult, NaiveDate};
use chrono_tz::Tz;
use clap::Parser;
use etherparse::{PacketBuilder, SlicedPacket};
use futures::channel::mpsc::UnboundedSender;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::try_join;
use log::info;
use pcap::{Capture, Device, Packet, PacketCodec};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use uapi;

use sunsniff::fields::{self, FIELDS};
#[cfg(feature = "influxdb2")]
use sunsniff::influxdb2::Influxdb2Receiver;
#[cfg(feature = "mqtt")]
use sunsniff::mqtt::MqttReceiver;
use sunsniff::receiver::{Receiver, Update};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    #[clap()]
    config_file: PathBuf,
}

/// Structure corresponding to the `[pcap]` section of the configuration file.
/// It is constructed from the config file by serde.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PcapConfig {
    device: String,
    #[serde(default)]
    file: bool,
    filter: Option<String>,
    timezone: Tz,
}

/// Structure corresponding to the configuration file. It is constructured
/// from the config file by serde.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    pcap: PcapConfig,
    #[cfg(feature = "influxdb2")]
    #[serde(default)]
    influxdb2: Vec<sunsniff::influxdb2::Config>,
    #[cfg(feature = "mqtt")]
    #[serde(default)]
    mqtt: Vec<sunsniff::mqtt::Config>,
}

struct Codec {
    pub tz: Tz,
}

/// Extract the timestamp from the packet.
///
/// The timestamp consists of YY-MM-DD HH:MM:SS in 6 one-byte fields, with
/// the year relative to 2000. It is in local time, so needs to be combined
/// with the timestamp.
///
/// If the timestamp is an invalid time, or is invalid or ambiguous for the
/// time zone, returns `None`.
fn parse_timestamp(payload: &[u8], tz: Tz) -> Option<DateTime<Tz>> {
    let dt = NaiveDate::from_ymd_opt(
        payload[fields::DATETIME_OFFSET] as i32 + 2000,
        payload[fields::DATETIME_OFFSET + 1] as u32,
        payload[fields::DATETIME_OFFSET + 2] as u32,
    )?
    .and_hms_opt(
        payload[fields::DATETIME_OFFSET + 3] as u32,
        payload[fields::DATETIME_OFFSET + 4] as u32,
        payload[fields::DATETIME_OFFSET + 5] as u32,
    )?
    .and_local_timezone(tz);
    match dt {
        LocalResult::Single(x) => Some(x),
        _ => None, // TODO: what to do with ambiguous times - try to guess based on history?
    }
}

fn tcp_reset(packet: &SlicedPacket) -> uapi::Result<()> {
    if let Some(etherparse::InternetSlice::Ipv4(ref orig_ip_header, _)) = packet.ip {
        if let Some(etherparse::TransportSlice::Tcp(ref orig_tcp_header)) = packet.transport {
            let builder = PacketBuilder::ipv4(
                orig_ip_header.destination(), // source IP address
                orig_ip_header.source(),      // destination IP address
                63,
            )
            .tcp(
                orig_tcp_header.destination_port(),      // source port
                orig_tcp_header.source_port(),           // destination port
                orig_tcp_header.acknowledgment_number(), // seq number
                1,                                       // window size
            )
            .rst();
            let mut data = vec![];
            let payload = [];
            builder.write(&mut data, &payload).unwrap();
            let sock = uapi::socket(uapi::c::AF_INET, uapi::c::SOCK_RAW, uapi::c::IPPROTO_RAW)?;
            let mut addr = uapi::pod_zeroed::<uapi::c::sockaddr_in>();
            addr.sin_family = uapi::c::AF_INET as u16;
            addr.sin_port = orig_tcp_header.source_port().to_be();
            addr.sin_addr.s_addr = u32::from_ne_bytes(orig_ip_header.source());
            // TODO: ideally should be non-blocking, but in reality it's unlikely
            // to block on a fresh socket
            uapi::sendto(sock.raw(), data.as_slice(), 0, &addr)?;
            info!("Sent TCP reset");
        }
    }
    Ok(())
}

impl PacketCodec for Codec {
    type Item = Option<Arc<Update<'static>>>;

    /// Decode a single packet
    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        if let Ok(sliced) = SlicedPacket::from_ethernet(packet.data) {
            if sliced.payload.len() == fields::MAGIC_LENGTH
                && sliced.payload[0] == fields::MAGIC_HEADER
            {
                let dt = match parse_timestamp(sliced.payload, self.tz) {
                    Some(x) => x,
                    None => {
                        return None; // Parse error means it's probably not the packet we expected
                    }
                };
                let serial =
                    std::str::from_utf8(&sliced.payload[fields::SERIAL_RANGE]).unwrap_or("unknown");
                info!(
                    "Received packet with timestamp {:?} for inverter {}",
                    dt, serial
                );
                let mut values = vec![];
                for field in FIELDS.iter() {
                    let bytes = &sliced.payload[field.offset..field.offset + 2];
                    let bytes = <&[u8; 2]>::try_from(bytes).unwrap();
                    let value = i16::from_be_bytes(*bytes);
                    let value = (value as f64) * field.scale + field.bias;
                    values.push(value);
                }
                let update = Update::new(dt.timestamp_nanos(), serial, FIELDS, values);

                tcp_reset(&sliced).unwrap(); // TODO: handle errors
                return Some(Arc::new(update));
            }
        }
        None
    }
}

/// Top-level execution. Receive updates from a stream and distribute them to
/// multiple receivers.
///
/// This is generic over the stream type so that it can support both live
/// capture and pcap files.
async fn run<S: Stream<Item = Result<<Codec as PacketCodec>::Item, pcap::Error>> + Unpin>(
    stream: &mut S,
    sinks: &mut [UnboundedSender<Arc<Update<'static>>>],
) -> Result<(), Box<dyn std::error::Error>> {
    while let Some(item) = stream.next().await {
        if let Some(update) = item? {
            for sink in sinks.iter_mut() {
                sink.unbounded_send(Arc::clone(&update))?;
            }
        }
    }
    for sink in sinks.iter_mut() {
        sink.close().await?; // TODO: do these in parallel?
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();
    let config = std::fs::read_to_string(args.config_file)?;
    let config: Config = toml::from_str(&config)?;

    let mut receivers: Vec<Box<dyn Receiver>> = vec![];
    #[cfg(feature = "influxdb2")]
    {
        for backend in config.influxdb2.iter() {
            receivers.push(Box::new(Influxdb2Receiver::new(backend).await));
        }
    }
    #[cfg(feature = "mqtt")]
    {
        for backend in config.mqtt.iter() {
            receivers.push(Box::new(MqttReceiver::new(backend)?));
        }
    }

    let mut sinks = vec![];
    let futures = FuturesUnordered::new();
    for receiver in receivers.iter_mut() {
        let (sink, stream) = futures::channel::mpsc::unbounded();
        futures.push(receiver.run(stream));
        sinks.push(sink);
    }

    let base_filter = "tcp";
    let filter = match &config.pcap.filter {
        Some(expr) => format!("({}) and ({})", base_filter, expr),
        None => String::from(base_filter),
    };

    // TODO: better handling of errors from receivers
    let codec = Codec {
        tz: config.pcap.timezone,
    };
    if config.pcap.file {
        let mut cap = Capture::from_file(&config.pcap.device)?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        /* cap.stream doesn't work on files. This is a somewhat hacky
         * workaround: it's probably going to load all the packets into
         * the sinks at once before giving them a chance to run.
         */
        let mut stream = futures::stream::iter(cap.iter(codec));
        try_join!(
            run(&mut stream, &mut sinks),
            futures.collect::<Vec<_>>().map(Ok)
        )?;
    } else {
        let device = Device::from(config.pcap.device.as_str());
        let cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        let mut cap = cap.setnonblock()?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        let mut stream = cap.stream(codec)?;
        try_join!(
            run(&mut stream, &mut sinks),
            futures.collect::<Vec<_>>().map(Ok)
        )?;
    }
    Ok(())
}
