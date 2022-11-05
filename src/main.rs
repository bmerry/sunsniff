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
use env_logger;
use etherparse::SlicedPacket;
use futures::channel::mpsc::UnboundedSender;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::try_join;
use log::info;
use pcap::{Capture, Device, Packet, PacketCodec};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

use sunsniff::influxdb2::Influxdb2Receiver;
use sunsniff::mqtt::MqttReceiver;
use sunsniff::receiver::{Field, Receiver, Update};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    #[clap()]
    config_file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PcapConfig {
    device: String,
    #[serde(default)]
    file: bool,
    filter: Option<String>,
    timezone: Tz,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    pcap: PcapConfig,
    #[serde(default)]
    influxdb2: Vec<sunsniff::influxdb2::Config>,
    #[serde(default)]
    mqtt: Vec<sunsniff::mqtt::Config>,
}

const MAGIC_LENGTH: usize = 292;
const MAGIC_HEADER: u8 = 0xa5;
const SERIAL_OFFSET: usize = 11;
const SERIAL_LENGTH: usize = 10;
const DATETIME_OFFSET: usize = 37;
const FIELDS: &[Field] = &[
    Field::energy(70, "Battery", "Total charge", "battery_charge_total"),
    Field::energy(74, "Battery", "Total discharge", "battery_discharge_total"),
    Field::energy(82, "Grid", "Total import", "grid_import_total"),
    Field::energy(88, "Grid", "Total export", "grid_export_total"),
    Field::frequency(84, "Grid", "grid_frequency"),
    Field::energy(96, "Load", "Total consumption", "load_consumption_total"),
    Field::temperature_name(106, "Inverter", "DC Temperature", "inverter_temperature_dc"),
    Field::temperature_name(108, "Inverter", "AC Temperature", "inverter_temperature_ac"),
    Field::energy(118, "PV", "Total production", "pv_production_total"),
    Field::new(
        140,
        "Battery",
        "Capacity",
        "battery_capacity",
        1.0,
        0.0,
        "Ah",
    ),
    Field::voltage(176, "Grid", "grid_voltage"),
    Field::voltage(184, "Load", "load_voltage"),
    Field::power(216, "Grid", "grid_power"),
    Field::power(228, "Load", "load_power"),
    Field::temperature(240, "Battery", "battery_temperature"),
    Field::new(244, "Battery", "SOC", "battery_soc", 1.0, 0.0, "%"),
    Field::power(248, "PV", "pv_power"),
    Field::power(256, "Battery", "battery_power"),
    Field::current(258, "Battery", "battery_current"),
    Field::frequency(260, "Load", "load_frequency"),
];

struct Codec {
    pub tz: Tz,
}

fn parse_timestamp(payload: &[u8], tz: Tz) -> Option<DateTime<Tz>> {
    let dt = NaiveDate::from_ymd_opt(
        payload[DATETIME_OFFSET + 0] as i32 + 2000,
        payload[DATETIME_OFFSET + 1] as u32,
        payload[DATETIME_OFFSET + 2] as u32,
    )?
    .and_hms_opt(
        payload[DATETIME_OFFSET + 3] as u32,
        payload[DATETIME_OFFSET + 4] as u32,
        payload[DATETIME_OFFSET + 5] as u32,
    )?
    .and_local_timezone(tz);
    match dt {
        LocalResult::Single(x) => Some(x),
        _ => None, // TODO: what to do with ambiguous times - try to guess based on history?
    }
}

impl PacketCodec for Codec {
    type Item = Option<Arc<Update<'static>>>;

    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        if let Ok(sliced) = SlicedPacket::from_ethernet(packet.data) {
            if sliced.payload.len() == MAGIC_LENGTH && sliced.payload[0] == MAGIC_HEADER {
                let dt = match parse_timestamp(sliced.payload, self.tz) {
                    Some(x) => x,
                    None => {
                        return None; // Parse error means it's probably not the packet we expected
                    }
                };
                let serial = std::str::from_utf8(
                    &sliced.payload[SERIAL_OFFSET..(SERIAL_OFFSET + SERIAL_LENGTH)],
                )
                .unwrap_or("unknown");
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
                return Some(Arc::new(update));
            }
        }
        None
    }
}

async fn run<S: Stream<Item = Result<<Codec as PacketCodec>::Item, pcap::Error>> + Unpin>(
    stream: &mut S,
    sinks: &mut [UnboundedSender<Arc<Update<'static>>>],
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        match stream.next().await {
            Some(item) => match item? {
                Some(update) => {
                    for sink in sinks.iter_mut() {
                        sink.unbounded_send(Arc::clone(&update))?;
                    }
                }
                None => {}
            },
            None => {
                break;
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
    for backend in config.influxdb2.iter() {
        receivers.push(Box::new(Influxdb2Receiver::new(&backend)));
    }
    for backend in config.mqtt.iter() {
        receivers.push(Box::new(MqttReceiver::new(&backend)?));
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
            futures.collect::<Vec<_>>().map(|x| Ok(x))
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
            futures.collect::<Vec<_>>().map(|x| Ok(x))
        )?;
    }
    Ok(())
}
